use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

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
    webhook_url: String,
    notify_pomodoro_end: bool,
    notify_break_end: bool,
    msg_pomodoro_end: String,
    msg_break_end: String,
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
            webhook_url: String::new(),
            notify_pomodoro_end: true,
            notify_break_end: true,
            msg_pomodoro_end: "🍅 Pomodoro #{count} terminé ! Place à la {next} ({min} min).".into(),
            msg_break_end: "⏰ {mode} terminée, retour au travail !".into(),
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
        storage()
            .and_then(|s| s.get_item(STORAGE_KEY).ok().flatten())
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        if let (Some(s), Ok(raw)) = (storage(), serde_json::to_string(self)) {
            let _ = s.set_item(STORAGE_KEY, &raw);
        }
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
            let (enabled, template) = match finished {
                Mode::Pomodoro => (s.notify_pomodoro_end, &s.msg_pomodoro_end),
                _ => (s.notify_break_end, &s.msg_break_end),
            };
            if enabled && !s.webhook_url.trim().is_empty() {
                let msg = template
                    .replace("{count}", &self.completed.get_untracked().to_string())
                    .replace("{mode}", finished.label())
                    .replace("{next}", next.label())
                    .replace("{min}", &s.minutes(next).to_string());
                self.send(msg);
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

    fn send(self, msg: String) {
        let url = self.settings.with_untracked(|s| s.webhook_url.trim().to_string());
        self.webhook_status.set("Envoi…".into());
        spawn_local(async move {
            let status = match send_webhook(&url, &msg).await {
                Ok(()) => "✓ Message envoyé".to_string(),
                Err(e) => format!("✗ Échec : {e}"),
            };
            self.webhook_status.set(status);
        });
    }
}

fn js_err(e: JsValue) -> String {
    e.as_string()
        .or_else(|| js_sys::JSON::stringify(&e).ok().and_then(|s| s.as_string()))
        .unwrap_or_else(|| "erreur inconnue".into())
}

/// POST to a Discord webhook. Sent as multipart form data (`payload_json`)
/// so the browser treats it as a simple request and skips the CORS preflight.
async fn send_webhook(url: &str, content: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("l'URL doit commencer par https://".into());
    }
    let form = web_sys::FormData::new().map_err(js_err)?;
    let payload = serde_json::json!({ "content": content }).to_string();
    form.append_with_str("payload_json", &payload).map_err(js_err)?;

    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    init.set_body(&form);

    let resp = JsFuture::from(window().fetch_with_str_and_init(url, &init))
        .await
        .map_err(js_err)?;
    let resp: web_sys::Response = resp.dyn_into().map_err(js_err)?;
    if resp.ok() {
        Ok(())
    } else {
        Err(format!("HTTP {}", resp.status()))
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

    let interval = set_interval_with_handle(move || timer.tick(), Duration::from_millis(250)).ok();
    on_cleanup(move || {
        if let Some(h) = interval {
            h.clear();
        }
    });

    // Persist settings, and apply new durations right away if the timer is idle.
    Effect::new(move |_| {
        let s = timer.settings.get();
        s.save();
        if !timer.running.get_untracked() {
            timer.remaining_ms.set(s.duration_ms(timer.mode.get_untracked()));
        }
    });

    Effect::new(move |_| {
        let title = format!("{} – {}", format_time(timer.remaining_ms.get()), timer.mode.get().label());
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
                <h1>"⏱ Pomodoro"</h1>
                <button class="icon-btn" on:click=move |_| show_settings.update(|v| *v = !*v)>
                    "⚙ Réglages"
                </button>
            </header>

            <main class="card">
                <nav class="tabs">{tab(Mode::Pomodoro)}{tab(Mode::Short)}{tab(Mode::Long)}</nav>
                <div class="time">{move || format_time(timer.remaining_ms.get())}</div>
                <div class="controls">
                    <button
                        class="start"
                        class:pressed=move || timer.running.get()
                        on:click=move |_| timer.toggle()
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

                <h3>"Webhook Discord"</h3>
                <label class="field wide">
                    <span>"URL du webhook"</span>
                    <input
                        type="password"
                        placeholder="https://discord.com/api/webhooks/…"
                        prop:value=move || s.with(|s| s.webhook_url.clone())
                        on:change=move |ev| s.update(|s| s.webhook_url = event_target_value(&ev).trim().to_string())
                    />
                </label>
                {check("Notifier à la fin d'un pomodoro", |s| s.notify_pomodoro_end, |s, v| s.notify_pomodoro_end = v)}
                {text("Message fin de pomodoro", |s| s.msg_pomodoro_end.clone(), |s, v| s.msg_pomodoro_end = v)}
                {check("Notifier à la fin d'une pause", |s| s.notify_break_end, |s, v| s.notify_break_end = v)}
                {text("Message fin de pause", |s| s.msg_break_end.clone(), |s, v| s.msg_break_end = v)}
                <p class="hint">
                    "Variables : {count} {mode} {next} {min}. L'URL reste stockée uniquement dans ce navigateur."
                </p>
                <div class="row">
                    <button
                        class="test"
                        disabled=move || s.with(|s| s.webhook_url.trim().is_empty())
                        on:click=move |_| timer.send("🔔 Test du webhook Pomodoro".into())
                    >
                        "Envoyer un test"
                    </button>
                    <span class="status">{move || timer.webhook_status.get()}</span>
                </div>

                <div class="row end">
                    <button class="reset" on:click=move |_| s.update(|s| {
                        *s = Settings { webhook_url: std::mem::take(&mut s.webhook_url), ..Settings::default() }
                    })>
                        "Réinitialiser"
                    </button>
                </div>
            </section>
        </div>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}
