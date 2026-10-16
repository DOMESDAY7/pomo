mod i18n;
mod notify;
mod tasks;
mod webhook;

use leptos::prelude::*;
use i18n::tr;
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
            Mode::Pomodoro => tr().pomodoro,
            Mode::Short => tr().short_break,
            Mode::Long => tr().long_break,
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
    /// Empty means the default message for the page language.
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
            msg_pomodoro_end: String::new(),
            msg_break_end: String::new(),
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
        for msg in [&mut s.msg_pomodoro_end, &mut s.msg_break_end] {
            if i18n::is_legacy_default(msg) {
                msg.clear();
            }
        }
        s
    }

    fn save(&self) {
        if let (Some(s), Ok(raw)) = (storage(), serde_json::to_string(self)) {
            let _ = s.set_item(STORAGE_KEY, &raw);
        }
    }

    fn message_template(&self, finished: Mode) -> &str {
        let (custom, default) = match finished {
            Mode::Pomodoro => (&self.msg_pomodoro_end, tr().default_msg_pomodoro),
            _ => (&self.msg_break_end, tr().default_msg_break),
        };
        if custom.trim().is_empty() { default } else { custom }
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
    tasks: RwSignal<tasks::TaskState>,
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
            tasks: RwSignal::new(tasks::TaskState::load()),
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
            if finished == Mode::Pomodoro {
                self.tasks.update(|t| t.on_pomodoro_done());
            }
            if s.sound {
                beep();
            }
            let webhooks_wanted = match finished {
                Mode::Pomodoro => s.notify_pomodoro_end,
                _ => s.notify_break_end,
            };
            let template = s.message_template(finished);
            let title = match finished {
                Mode::Pomodoro => tr().pomodoro_done,
                _ => tr().break_done,
            };
            let mut vars: Vars = vec![
                ("count", self.completed.get_untracked().to_string()),
                ("mode", finished.label().to_string()),
                ("next", next.label().to_string()),
                ("min", s.minutes(next).to_string()),
                ("task", self.active_task_title()),
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
                    self.webhook_status.set(tr().offline_skipped.into());
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

    fn active_task_title(self) -> String {
        self.tasks
            .with_untracked(|s| s.active_task().map(|t| t.title.clone()))
            .unwrap_or_default()
    }

    fn send_all(self, hooks: Vec<Webhook>, title: String, msg: String, vars: Vars) {
        self.webhook_status.set(tr().sending.into());
        spawn_local(async move {
            let mut lines = Vec::new();
            for hook in &hooks {
                let result = match webhook::send(hook, &title, &msg, &vars).await {
                    Ok(Delivery::Confirmed) => tr().sent.to_string(),
                    Ok(Delivery::Sent) => tr().sent_opaque.to_string(),
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
            self.webhook_status.set(tr().offline_no_test.into());
            return;
        }
        let title = tr().test_title.to_string();
        let msg = tr().test_message.to_string();
        let vars: Vars = vec![
            ("title", title.clone()),
            ("message", msg.clone()),
            ("count", self.completed.get_untracked().to_string()),
            ("mode", self.mode.get_untracked().label().to_string()),
            ("next", self.next_mode().label().to_string()),
            ("min", "0".into()),
            ("task", self.active_task_title()),
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
    Effect::new(move |_| timer.tasks.with(|t| t.save()));

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
                <div class="header-actions">
                    <a class="icon-btn" href=tr().switch_href title=tr().switch_title hreflang=match i18n::lang() {
                        i18n::Lang::Fr => "en",
                        i18n::Lang::En => "fr",
                    }>
                        {tr().switch_label}
                    </a>
                    <button class="icon-btn" on:click=move |_| show_settings.update(|v| *v = !*v)>
                        {tr().settings}
                    </button>
                </div>
            </header>

            <Show when=move || !online.get()>
                <p class="offline">{tr().offline_banner}</p>
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
                        <button class="skip" title=tr().skip on:click=move |_| timer.finish(false)>
                            "⏭"
                        </button>
                    </Show>
                </div>
            </main>

            <p class="count">
                {move || format!("#{}", timer.completed.get() + 1)}
                <br />
                {move || tasks::focus_label(timer)}
            </p>

            <tasks::TaskSection timer=timer />

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

    let text = move |label: &'static str, placeholder: &'static str, get: fn(&Settings) -> String, set: fn(&mut Settings, String)| {
        view! {
            <label class="field wide">
                <span>{label}</span>
                <input
                    type="text"
                    placeholder=placeholder
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
                    <h2>{tr().settings.trim_start_matches("⚙ ")}</h2>
                    <button class="close" on:click=move |_| on_close()>"✕"</button>
                </div>

                <h3>{tr().durations}</h3>
                <div class="grid">
                    {num(tr().pomodoro, |s| s.pomodoro_min, |s, v| s.pomodoro_min = v)}
                    {num(tr().short_break, |s| s.short_min, |s, v| s.short_min = v)}
                    {num(tr().long_break, |s| s.long_min, |s, v| s.long_min = v)}
                </div>
                {num(tr().long_interval, |s| s.long_interval, |s, v| s.long_interval = v)}
                {check(tr().auto_breaks, |s| s.auto_start_breaks, |s, v| s.auto_start_breaks = v)}
                {check(tr().auto_pomodoros, |s| s.auto_start_pomodoros, |s, v| s.auto_start_pomodoros = v)}
                {check(tr().sound, |s| s.sound, |s, v| s.sound = v)}

                <h3>{tr().browser_notifications}</h3>
                <label class="toggle">
                    <span>{tr().notify_each_session}</span>
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
                                notify::show("🔔 Pomodoro", tr().notif_test_body);
                            })
                        }
                    >
                        {move || if perm.get() == Permission::Granted { tr().test } else { tr().allow }}
                    </button>
                    <span class="status">
                        {move || match perm.get() {
                            Permission::Granted => tr().perm_granted,
                            Permission::Denied => tr().perm_denied,
                            Permission::Unsupported => tr().perm_unsupported,
                            Permission::Default => tr().perm_default,
                        }}
                    </span>
                </div>
                <p class="hint">{tr().offline_hint}</p>

                <h3>{tr().webhooks}</h3>
                {check(tr().send_on_pomodoro_end, |s| s.notify_pomodoro_end, |s, v| s.notify_pomodoro_end = v)}
                {text(tr().msg_pomodoro_label, tr().default_msg_pomodoro, |s| s.msg_pomodoro_end.clone(), |s, v| s.msg_pomodoro_end = v)}
                {check(tr().send_on_break_end, |s| s.notify_break_end, |s, v| s.notify_break_end = v)}
                {text(tr().msg_break_label, tr().default_msg_break, |s| s.msg_break_end.clone(), |s, v| s.msg_break_end = v)}
                <p class="hint">{tr().variables_hint}</p>

                <For
                    each=move || s.with(|s| s.webhooks.iter().map(|w| w.id).collect::<Vec<_>>())
                    key=|id| *id
                    children=move |id| view! { <WebhookEditor timer=timer id=id /> }
                />
                <div class="row">
                    <button class="add" on:click=add_webhook>{tr().add_webhook}</button>
                </div>
                <p class="status">{move || timer.webhook_status.get()}</p>

                <div class="row end">
                    <button class="reset" on:click=move |_| s.update(|s| {
                        *s = Settings { webhooks: std::mem::take(&mut s.webhooks), ..Settings::default() }
                    })>
                        {tr().reset}
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
                    {tr().enabled}
                </label>
                <button class="close" title=tr().delete on:click=move |_| s.update(|s| s.webhooks.retain(|w| w.id != id))>
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
                        <span>{tr().method}</span>
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
                    <span>{tr().body}</span>
                    <textarea
                        rows="3"
                        prop:value=move || get(|w| w.body.clone())
                        on:change=move |ev| set(|w, v| w.body = v, event_target_value(&ev))
                    />
                </label>
                <p class="hint">{tr().cors_hint}</p>
            </Show>
            <button
                class="test"
                disabled=move || get(|w| w.url.clone()).trim().is_empty()
                on:click=move |_| timer.test_webhook(id)
            >
                {tr().send_test}
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
