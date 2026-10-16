mod notify;
mod webhook;

use leptos::prelude::*;
use leptos::task::spawn_local;
use notify::Permission;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use wasm_bindgen::JsCast;
use webhook::{Delivery, Kind, Vars, Webhook};

const STORAGE_KEY: &str = "pomo.settings";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Pomodoro,
    Short,
    Long,
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Mode::Pomodoro => "Pomodoro",
            Mode::Short => "Short Break",
            Mode::Long => "Long Break",
        }
    }

    fn class(self) -> &'static str {
        match self {
            Mode::Pomodoro => "mode-pomodoro",
            Mode::Short => "mode-short",
            Mode::Long => "mode-long",
        }
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    pomodoro_min: u32,
    short_min: u32,
    long_min: u32,
    long_interval: u32,
    auto_start_breaks: bool,
    auto_start_pomodoros: bool,
    sound: bool,
    browser_notify: bool,
    webhooks: Vec<Webhook>,
    notify_pomodoro_end: bool,
    notify_break_end: bool,
    msg_pomodoro_end: String,
    msg_break_end: String,
    /// Pre-multi-webhook setting, migrated to a Discord entry on load.
    #[serde(skip_serializing)]
    webhook_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            pomodoro_min: 25,
            short_min: 5,
            long_min: 15,
            long_interval: 4,
            auto_start_breaks: false,
            auto_start_pomodoros: false,
            sound: true,
            browser_notify: true,
            webhooks: Vec::new(),
            notify_pomodoro_end: true,
            notify_break_end: true,
            msg_pomodoro_end: "🍅 Pomodoro #{count} terminé ! Place à la {next} ({min} min).".into(),
            msg_break_end: "⏰ {mode} terminée, retour au travail !".into(),
            webhook_url: String::new(),
        }
    }
}

impl Settings {
    fn minutes(&self, mode: Mode) -> u32 {
        match mode {
            Mode::Pomodoro => self.pomodoro_min,
            Mode::Short => self.short_min,
            Mode::Long => self.long_min,
        }
    }

    fn duration_ms(&self, mode: Mode) -> f64 {
        self.minutes(mode).max(1) as f64 * 60_000.0
    }

    fn load() -> Self {
        let mut s: Settings = storage()
            .and_then(|s| s.get_item(STORAGE_KEY).ok().flatten())
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        let legacy = std::mem::take(&mut s.webhook_url);
        if !legacy.trim().is_empty() && s.webhooks.is_empty() {
            s.webhooks.push(Webhook { id: 1, url: legacy, ..Webhook::default() });
        }
        s
    }

    fn save(&self) {
        if let (Some(s), Ok(raw)) = (storage(), serde_json::to_string(self)) {
            let _ = s.set_item(STORAGE_KEY, &raw);
        }
    }

    fn webhook(&self, id: u32) -> Option<&Webhook> {
        self.webhooks.iter().find(|w| w.id == id)
    }

    fn webhook_mut(&mut self, id: u32) -> Option<&mut Webhook> {
        self.webhooks.iter_mut().find(|w| w.id == id)
    }
}

fn storage() -> Option<web_sys::Storage> {
    window().local_storage().ok().flatten()
}

fn now() -> f64 {
    js_sys::Date::now()
}

fn format_time(ms: f64) -> String {
    let total = (ms / 1000.0).ceil().max(0.0) as u64;
    format!("{:02}:{:02}", total / 60, total % 60)
}

/// Shared timer state. Every field is a Copy handle, so the struct is Copy too.
#[derive(Clone, Copy)]
struct Timer {
    settings: RwSignal<Settings>,
    mode: RwSignal<Mode>,
    running: RwSignal<bool>,
    remaining_ms: RwSignal<f64>,
    end_at: StoredValue<f64>,
    completed: RwSignal<u32>,
    webhook_status: RwSignal<String>,
}

impl Timer {
    fn new() -> Self {
        let settings = Settings::load();
        let remaining = settings.duration_ms(Mode::Pomodoro);
        Self {
            settings: RwSignal::new(settings),
            mode: RwSignal::new(Mode::Pomodoro),
            running: RwSignal::new(false),
            remaining_ms: RwSignal::new(remaining),
            end_at: StoredValue::new(0.0),
            completed: RwSignal::new(0),
            webhook_status: RwSignal::new(String::new()),
        }
    }

    fn start(self) {
        self.end_at.set_value(now() + self.remaining_ms.get_untracked());
        self.running.set(true);
    }

    fn pause(self) {
        self.tick();
        self.running.set(false);
    }

    fn toggle(self) {
        if self.running.get_untracked() {
            self.pause()
        } else {
            self.start()
        }
    }

    fn set_mode(self, mode: Mode) {
        self.running.set(false);
        self.mode.set(mode);
        self.remaining_ms
            .set(self.settings.with_untracked(|s| s.duration_ms(mode)));
    }

    fn tick(self) {
        if !self.running.get_untracked() {
            return;
        }
        let left = self.end_at.get_value() - now();
        if left <= 0.0 {
            self.finish(true);
        } else {
            self.remaining_ms.set(left);
        }
    }

    fn next_mode(self) -> Mode {
        match self.mode.get_untracked() {
            Mode::Pomodoro => {
                let interval = self.settings.with_untracked(|s| s.long_interval.max(1));
                if (self.completed.get_untracked() + 1) % interval == 0 {
                    Mode::Long
                } else {
                    Mode::Short
                }
            }
            _ => Mode::Pomodoro,
        }
    }

    /// End the current session. `natural` is false when the user skipped it.
    fn finish(self, natural: bool) {
        let finished = self.mode.get_untracked();
        let next = self.next_mode();
        if finished == Mode::Pomodoro {
            self.completed.update(|c| *c += 1);
        }
        let s = self.settings.get_untracked();

        if natural {
            if s.sound {
                beep();
            }
            let (webhooks_wanted, template) = match finished {
                Mode::Pomodoro => (s.notify_pomodoro_end, &s.msg_pomodoro_end),
                _ => (s.notify_break_end, &s.msg_break_end),
            };
            let title = match finished {
                Mode::Pomodoro => "🍅 Pomodoro terminé",
                _ => "⏰ Pause terminée",
            };
            let mut vars: Vars = vec![
                ("count", self.completed.get_untracked().to_string()),
                ("mode", finished.label().to_string()),
                ("next", next.label().to_string()),
                ("min", s.minutes(next).to_string()),
            ];
            let msg = webhook::render(template, &vars, str::to_string);
            vars.push(("title", title.to_string()));
            vars.push(("message", msg.clone()));

            let hooks: Vec<Webhook> = s
                .webhooks
                .iter()
                .filter(|w| w.enabled && !w.url.trim().is_empty())
                .cloned()
                .collect();
            let wants_webhooks = webhooks_wanted && !hooks.is_empty();
            let online = notify::is_online();

            // Offline: webhooks can't go out, so the browser notification is
            // the only channel left and is shown even if it was turned off.
            if s.browser_notify || (wants_webhooks && !online) {
                notify::show(title, &msg);
            }
            if wants_webhooks {
                if online {
                    self.send_all(hooks, title.to_string(), msg, vars);
                } else {
                    self.webhook_status
                        .set("Hors ligne : webhooks ignorés, notification navigateur seulement".into());
                }
            }
        }

        self.set_mode(next);
        let auto = match next {
            Mode::Pomodoro => s.auto_start_pomodoros,
            _ => s.auto_start_breaks,
        };
        if natural && auto {
            self.start();
        }
    }

    fn send_all(self, hooks: Vec<Webhook>, title: String, msg: String, vars: Vars) {
        self.webhook_status.set("Envoi…".into());
        spawn_local(async move {
            let mut lines = Vec::new();
            for hook in &hooks {
                let result = match webhook::send(hook, &title, &msg, &vars).await {
                    Ok(Delivery::Confirmed) => "✓ envoyé".to_string(),
                    Ok(Delivery::Sent) => "✓ envoyé (réponse non lisible)".to_string(),
                    Err(e) => format!("✗ {e}"),
                };
                lines.push(format!("{} : {result}", hook.kind.label()));
            }
            self.webhook_status.set(lines.join(" · "));
        });
    }

    fn test_webhook(self, id: u32) {
        let Some(hook) = self.settings.with_untracked(|s| s.webhook(id).cloned()) else { return };
        if !notify::is_online() {
            self.webhook_status.set("Hors ligne : impossible de tester".into());
            return;
        }
        let title = "🔔 Test Pomodoro".to_string();
        let msg = "Test du webhook Pomodoro".to_string();
        let vars: Vars = vec![
            ("title", title.clone()),
            ("message", msg.clone()),
            ("count", self.completed.get_untracked().to_string()),
            ("mode", self.mode.get_untracked().label().to_string()),
            ("next", self.next_mode().label().to_string()),
            ("min", "0".into()),
        ];
        self.send_all(vec![hook], title, msg, vars);
    }
}

fn beep() {
    let Ok(ctx) = web_sys::AudioContext::new() else { return };
    let t0 = ctx.current_time();
    for i in 0..3 {
        let t = t0 + i as f64 * 0.3;
        let (Ok(osc), Ok(gain)) = (ctx.create_oscillator(), ctx.create_gain()) else { return };
        osc.frequency().set_value(880.0);
        let _ = gain.gain().set_value_at_time(0.2, t);
        let _ = gain.gain().exponential_ramp_to_value_at_time(0.001, t + 0.2);
        let _ = osc.connect_with_audio_node(&gain);
        let _ = gain.connect_with_audio_node(&ctx.destination());
        let _ = osc.start_with_when(t);
        let _ = osc.stop_with_when(t + 0.2);
    }
}

#[component]
fn App() -> impl IntoView {
    let timer = Timer::new();
    let show_settings = RwSignal::new(false);
    let online = RwSignal::new(notify::is_online());

    let interval = set_interval_with_handle(move || timer.tick(), Duration::from_millis(250)).ok();
    on_cleanup(move || {
        if let Some(h) = interval {
            h.clear();
        }
    });
    let _ = window_event_listener(leptos::ev::online, move |_| online.set(true));
    let _ = window_event_listener(leptos::ev::offline, move |_| online.set(false));

    // Persist settings, and apply new durations right away if the timer is idle.
    Effect::new(move |_| {
        let s = timer.settings.get();
        s.save();
        if !timer.running.get_untracked() {
            timer.remaining_ms.set(s.duration_ms(timer.mode.get_untracked()));
        }
    });

    // Show the countdown in the tab title while running; keep the SEO title
    // from index.html otherwise so crawlers that render JS index the real one.
    let base_title = document().title();
    Effect::new(move |_| {
        let title = if timer.running.get() {
            format!("{} – {}", format_time(timer.remaining_ms.get()), timer.mode.get().label())
        } else {
            base_title.clone()
        };
        document().set_title(&title);
    });

    let tab = move |mode: Mode| {
        view! {
            <button
                class="tab"
                class:active=move || timer.mode.get() == mode
                on:click=move |_| timer.set_mode(mode)
            >
                {mode.label()}
            </button>
        }
    };

    view! {
        <div class=move || format!("app {}", timer.mode.get().class())>
            <header>
                <div class="brand"><img class="logo" src="icons/icon-192.png" alt="" />"Pomodoro"</div>
                <button class="icon-btn" on:click=move |_| show_settings.update(|v| *v = !*v)>
                    "⚙ Réglages"
                </button>
            </header>

            <Show when=move || !online.get()>
                <p class="offline">"Hors ligne : seules les notifications du navigateur fonctionnent"</p>
            </Show>

            <main class="card">
                <nav class="tabs">{tab(Mode::Pomodoro)}{tab(Mode::Short)}{tab(Mode::Long)}</nav>
                <div class="time">{move || format_time(timer.remaining_ms.get())}</div>
                <div class="controls">
                    <button
                        class="start"
                        class:pressed=move || timer.running.get()
                        on:click=move |_| {
                            if timer.settings.with_untracked(|s| s.browser_notify) {
                                notify::request(|_| {});
                            }
                            timer.toggle()
                        }
                    >
                        {move || if timer.running.get() { "PAUSE" } else { "START" }}
                    </button>
                    <Show when=move || timer.running.get()>
                        <button class="skip" title="Passer" on:click=move |_| timer.finish(false)>
                            "⏭"
                        </button>
                    </Show>
                </div>
            </main>

            <p class="count">
                {move || format!("#{}", timer.completed.get() + 1)}
                <br />
                {move || if timer.mode.get() == Mode::Pomodoro { "Au travail !" } else { "C'est la pause !" }}
            </p>

            <Show when=move || show_settings.get()>
                <SettingsPanel timer=timer on_close=move || show_settings.set(false) />
            </Show>
        </div>
    }
}

#[component]
fn SettingsPanel(timer: Timer, on_close: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let s = timer.settings;
    let perm = RwSignal::new(notify::permission());

    let num = move |label: &'static str, get: fn(&Settings) -> u32, set: fn(&mut Settings, u32)| {
        view! {
            <label class="field">
                <span>{label}</span>
                <input
                    type="number"
                    min="1"
                    max="999"
                    prop:value=move || s.with(|s| get(s)).to_string()
                    on:change=move |ev| {
                        if let Ok(v) = event_target_value(&ev).parse::<u32>() {
                            s.update(|s| set(s, v.clamp(1, 999)));
                        }
                    }
                />
            </label>
        }
    };

    let check = move |label: &'static str, get: fn(&Settings) -> bool, set: fn(&mut Settings, bool)| {
        view! {
            <label class="toggle">
                <span>{label}</span>
                <input
                    type="checkbox"
                    prop:checked=move || s.with(|s| get(s))
                    on:change=move |ev| s.update(|s| set(s, event_target_checked(&ev)))
                />
            </label>
        }
    };

    let text = move |label: &'static str, get: fn(&Settings) -> String, set: fn(&mut Settings, String)| {
        view! {
            <label class="field wide">
                <span>{label}</span>
                <input
                    type="text"
                    prop:value=move || s.with(|s| get(s))
                    on:change=move |ev| s.update(|s| set(s, event_target_value(&ev)))
                />
            </label>
        }
    };

    let add_webhook = move |_| {
        s.update(|s| {
            let id = s.webhooks.iter().map(|w| w.id).max().unwrap_or(0) + 1;
            s.webhooks.push(Webhook { id, ..Webhook::default() });
        })
    };

    view! {
        <div class="overlay" on:click=move |_| on_close()>
            <section class="panel" on:click=|ev| ev.stop_propagation()>
                <div class="panel-head">
                    <h2>"Réglages"</h2>
                    <button class="close" on:click=move |_| on_close()>"✕"</button>
                </div>

                <h3>"Durées (minutes)"</h3>
                <div class="grid">
                    {num("Pomodoro", |s| s.pomodoro_min, |s, v| s.pomodoro_min = v)}
                    {num("Short Break", |s| s.short_min, |s, v| s.short_min = v)}
                    {num("Long Break", |s| s.long_min, |s, v| s.long_min = v)}
                </div>
                {num("Pause longue tous les N pomodoros", |s| s.long_interval, |s, v| s.long_interval = v)}
                {check("Démarrer les pauses automatiquement", |s| s.auto_start_breaks, |s, v| s.auto_start_breaks = v)}
                {check("Démarrer les pomodoros automatiquement", |s| s.auto_start_pomodoros, |s, v| s.auto_start_pomodoros = v)}
                {check("Son à la fin", |s| s.sound, |s, v| s.sound = v)}

                <h3>"Notifications du navigateur"</h3>
                <label class="toggle">
                    <span>"Notifier à la fin de chaque session"</span>
                    <input
                        type="checkbox"
                        prop:checked=move || s.with(|s| s.browser_notify)
                        on:change=move |ev| {
                            let on = event_target_checked(&ev);
                            s.update(|s| s.browser_notify = on);
                            if on {
                                notify::request(move |p| perm.set(p));
                            }
                        }
                    />
                </label>
                <div class="row">
                    <button
                        class="test"
                        disabled=move || matches!(perm.get(), Permission::Unsupported | Permission::Denied)
                        on:click=move |_| {
                            notify::request(move |p| {
                                perm.set(p);
                                notify::show("🔔 Pomodoro", "Les notifications fonctionnent !");
                            })
                        }
                    >
                        {move || if perm.get() == Permission::Granted { "Tester" } else { "Autoriser" }}
                    </button>
                    <span class="status">
                        {move || match perm.get() {
                            Permission::Granted => "✓ Autorisées",
                            Permission::Denied => "✗ Bloquées : autorise-les dans les réglages du site",
                            Permission::Unsupported => "Non supportées par ce navigateur",
                            Permission::Default => "Permission pas encore demandée",
                        }}
                    </span>
                </div>
                <p class="hint">"Sans connexion, les webhooks sont ignorés et la notification du navigateur est toujours affichée."</p>

                <h3>"Webhooks"</h3>
                {check("Envoyer à la fin d'un pomodoro", |s| s.notify_pomodoro_end, |s, v| s.notify_pomodoro_end = v)}
                {text("Message fin de pomodoro", |s| s.msg_pomodoro_end.clone(), |s, v| s.msg_pomodoro_end = v)}
                {check("Envoyer à la fin d'une pause", |s| s.notify_break_end, |s, v| s.notify_break_end = v)}
                {text("Message fin de pause", |s| s.msg_break_end.clone(), |s, v| s.msg_break_end = v)}
                <p class="hint">
                    "Variables : {count} {mode} {next} {min}, plus {title} et {message} dans un corps personnalisé. Les URLs restent stockées uniquement dans ce navigateur."
                </p>

                <For
                    each=move || s.with(|s| s.webhooks.iter().map(|w| w.id).collect::<Vec<_>>())
                    key=|id| *id
                    children=move |id| view! { <WebhookEditor timer=timer id=id /> }
                />
                <div class="row">
                    <button class="add" on:click=add_webhook>"+ Ajouter un webhook"</button>
                </div>
                <p class="status">{move || timer.webhook_status.get()}</p>

                <div class="row end">
                    <button class="reset" on:click=move |_| s.update(|s| {
                        *s = Settings { webhooks: std::mem::take(&mut s.webhooks), ..Settings::default() }
                    })>
                        "Réinitialiser"
                    </button>
                </div>
            </section>
        </div>
    }
}

#[component]
fn WebhookEditor(timer: Timer, id: u32) -> impl IntoView {
    let s = timer.settings;
    let get = move |f: fn(&Webhook) -> String| s.with(|s| s.webhook(id).map(f).unwrap_or_default());
    let set = move |f: fn(&mut Webhook, String), v: String| {
        s.update(|s| {
            if let Some(w) = s.webhook_mut(id) {
                f(w, v)
            }
        })
    };
    let kind = move || s.with(|s| s.webhook(id).map(|w| w.kind).unwrap_or(Kind::Custom));

    view! {
        <div class="hook">
            <div class="hook-head">
                <select
                    prop:value=move || kind().id()
                    on:change=move |ev| set(|w, v| w.kind = Kind::from_id(&v), event_target_value(&ev))
                >
                    {Kind::ALL.into_iter().map(|k| view! { <option value=k.id()>{k.label()}</option> }).collect_view()}
                </select>
                <label class="mini">
                    <input
                        type="checkbox"
                        prop:checked=move || s.with(|s| s.webhook(id).is_some_and(|w| w.enabled))
                        on:change=move |ev| {
                            let on = event_target_checked(&ev);
                            s.update(|s| if let Some(w) = s.webhook_mut(id) { w.enabled = on });
                        }
                    />
                    "Actif"
                </label>
                <button class="close" title="Supprimer" on:click=move |_| s.update(|s| s.webhooks.retain(|w| w.id != id))>
                    "🗑"
                </button>
            </div>
            <input
                class="hook-url"
                type="password"
                placeholder=move || kind().placeholder()
                prop:value=move || get(|w| w.url.clone())
                on:change=move |ev| set(|w, v| w.url = v.trim().to_string(), event_target_value(&ev))
            />
            <Show when=move || kind() == Kind::Custom>
                <div class="grid two">
                    <label class="field">
                        <span>"Méthode"</span>
                        <select
                            prop:value=move || get(|w| w.method.clone())
                            on:change=move |ev| set(|w, v| w.method = v, event_target_value(&ev))
                        >
                            <option value="POST">"POST"</option>
                            <option value="PUT">"PUT"</option>
                            <option value="PATCH">"PATCH"</option>
                        </select>
                    </label>
                    <label class="field">
                        <span>"Content-Type"</span>
                        <input
                            type="text"
                            list="content-types"
                            prop:value=move || get(|w| w.content_type.clone())
                            on:change=move |ev| set(|w, v| w.content_type = v, event_target_value(&ev))
                        />
                        <datalist id="content-types">
                            <option value="application/json" />
                            <option value="text/plain" />
                            <option value="application/x-www-form-urlencoded" />
                        </datalist>
                    </label>
                </div>
                <label class="field wide">
                    <span>"Corps"</span>
                    <textarea
                        rows="3"
                        prop:value=move || get(|w| w.body.clone())
                        on:change=move |ev| set(|w, v| w.body = v, event_target_value(&ev))
                    />
                </label>
                <p class="hint">
                    "Hors text/plain et form-urlencoded, le serveur doit autoriser le CORS depuis cette page."
                </p>
            </Show>
            <button
                class="test"
                disabled=move || get(|w| w.url.clone()).trim().is_empty()
                on:click=move |_| timer.test_webhook(id)
            >
                "Envoyer un test"
            </button>
        </div>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    let root = document()
        .get_element_by_id("app")
        .expect("#app missing from index.html");
    // The static <noscript> fallback is only for non-JS visitors.
    root.set_inner_html("");
    leptos::mount::mount_to(root.unchecked_into(), App).forget();
}
