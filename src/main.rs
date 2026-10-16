mod audio;
mod i18n;
mod notify;
mod pip;
mod stats;
mod tasks;
mod webhook;

use audio::{Alarm, Noise};
use i18n::{tr, Lang};
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

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
enum Theme {
    #[default]
    Auto,
    Light,
    Dark,
}

impl Theme {
    const ALL: [Theme; 3] = [Theme::Auto, Theme::Light, Theme::Dark];

    fn id(self) -> &'static str {
        match self {
            Theme::Auto => "auto",
            Theme::Light => "light",
            Theme::Dark => "dark",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Theme::Auto => tr().theme_auto,
            Theme::Light => tr().theme_light,
            Theme::Dark => tr().theme_dark,
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
    alarm: Alarm,
    volume: u32,
    tick: bool,
    noise: Noise,
    noise_volume: u32,
    theme: Theme,
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
    /// Pre-alarm-choice on/off switch, migrated to `alarm` on load.
    #[serde(skip_serializing)]
    sound: bool,
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
            alarm: Alarm::Beep,
            volume: 70,
            tick: false,
            noise: Noise::None,
            noise_volume: 40,
            theme: Theme::Auto,
            browser_notify: true,
            webhooks: Vec::new(),
            notify_pomodoro_end: true,
            notify_break_end: true,
            msg_pomodoro_end: String::new(),
            msg_break_end: String::new(),
            webhook_url: String::new(),
            sound: true,
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
        if !s.sound {
            s.alarm = Alarm::None;
            s.sound = true;
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

/// `?overlay=1` turns the page into a transparent timer for OBS browser
/// sources. Its settings come from the URL and are never saved, so an
/// overlay can't overwrite the real app's settings in the same browser.
#[derive(Clone)]
struct OverlayConfig {
    settings: Settings,
    autostart: bool,
    label: String,
}

fn overlay_config() -> Option<OverlayConfig> {
    let search = window().location().search().ok()?;
    let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
    if params.get("overlay").as_deref() != Some("1") {
        return None;
    }
    let num = |k: &str, d: u32| params.get(k).and_then(|v| v.parse::<u32>().ok()).map_or(d, |v| v.clamp(1, 999));
    let flag = |k: &str| params.get(k).as_deref() == Some("1");
    let autostart = flag("autostart");
    let settings = Settings {
        pomodoro_min: num("pomo", 25),
        short_min: num("short", 5),
        long_min: num("long", 15),
        long_interval: num("interval", 4),
        auto_start_breaks: autostart,
        auto_start_pomodoros: autostart,
        alarm: if flag("sound") { Alarm::Beep } else { Alarm::None },
        browser_notify: false,
        ..Settings::default()
    };
    Some(OverlayConfig { settings, autostart, label: params.get("label").unwrap_or_default() })
}

/// Shared timer state. Every field is a Copy handle, so the struct is Copy too.
#[derive(Clone, Copy)]
struct Timer {
    settings: RwSignal<Settings>,
    mode: RwSignal<Mode>,
    running: RwSignal<bool>,
    remaining_ms: RwSignal<f64>,
    end_at: StoredValue<f64>,
    last_tick_sec: StoredValue<u64>,
    completed: RwSignal<u32>,
    webhook_status: RwSignal<String>,
    tasks: RwSignal<tasks::TaskState>,
    /// False for the OBS overlay: no saving, history or webhooks.
    persist: bool,
}

impl Timer {
    fn new(settings: Settings, persist: bool) -> Self {
        let remaining = settings.duration_ms(Mode::Pomodoro);
        Self {
            settings: RwSignal::new(settings),
            mode: RwSignal::new(Mode::Pomodoro),
            running: RwSignal::new(false),
            remaining_ms: RwSignal::new(remaining),
            end_at: StoredValue::new(0.0),
            last_tick_sec: StoredValue::new(0),
            completed: RwSignal::new(0),
            webhook_status: RwSignal::new(String::new()),
            tasks: RwSignal::new(if persist { tasks::TaskState::load() } else { tasks::TaskState::default() }),
            persist,
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
        audio::unlock();
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
            return;
        }
        self.remaining_ms.set(left);

        let sec = (left / 1000.0).ceil() as u64;
        if sec != self.last_tick_sec.get_value() {
            self.last_tick_sec.set_value(sec);
            let (tick, volume) = self.settings.with_untracked(|s| (s.tick, s.volume));
            if tick && self.mode.get_untracked() == Mode::Pomodoro {
                audio::tick(sec % 2 == 0, volume);
            }
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
            audio::play_alarm(s.alarm, s.volume);
        }
        if natural && self.persist {
            if finished == Mode::Pomodoro {
                stats::record(s.pomodoro_min, self.active_task_title());
                self.tasks.update(|t| t.on_pomodoro_done());
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

/// Run the timer loop for the lifetime of the component.
fn run_ticker(timer: Timer) {
    let interval = set_interval_with_handle(move || timer.tick(), Duration::from_millis(250)).ok();
    on_cleanup(move || {
        if let Some(h) = interval {
            h.clear();
        }
    });
}

/// True when a key press is meant for a form field, not for a shortcut.
fn typing_in_field(ev: &web_sys::KeyboardEvent) -> bool {
    ev.target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        .is_some_and(|el| {
            matches!(el.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
                || el.get_attribute("contenteditable").is_some()
        })
}

#[component]
fn OverlayApp(cfg: OverlayConfig) -> impl IntoView {
    let timer = Timer::new(cfg.settings, false);
    run_ticker(timer);
    if cfg.autostart {
        timer.start();
    }
    let _ = window_event_listener(leptos::ev::keydown, move |ev| {
        if ev.key() == " " {
            ev.prevent_default();
            timer.toggle();
        }
    });
    let label = cfg.label;

    view! {
        <div class=move || format!("obs {}", timer.mode.get().class()) on:click=move |_| timer.toggle()>
            <div class="obs-session">{move || timer.mode.get().label()}</div>
            <div class="obs-time">{move || format_time(timer.remaining_ms.get())}</div>
            {(!label.is_empty()).then(|| view! { <div class="obs-label">{label.clone()}</div> })}
        </div>
    }
}

#[component]
fn App() -> impl IntoView {
    let timer = Timer::new(Settings::load(), true);
    let show_settings = RwSignal::new(false);
    let show_stats = RwSignal::new(false);
    let show_help = RwSignal::new(false);
    let task_editing: tasks::Editing = RwSignal::new(None);
    let online = RwSignal::new(notify::is_online());
    let pip_ok = pip::supported();

    run_ticker(timer);
    let _ = window_event_listener(leptos::ev::online, move |_| online.set(true));
    let _ = window_event_listener(leptos::ev::offline, move |_| online.set(false));

    let _ = window_event_listener(leptos::ev::keydown, move |ev| {
        if ev.key() == "Escape" {
            show_settings.set(false);
            show_stats.set(false);
            show_help.set(false);
            task_editing.set(None);
            return;
        }
        if ev.ctrl_key() || ev.meta_key() || ev.alt_key() || typing_in_field(&ev) {
            return;
        }
        let handled = match ev.key().as_str() {
            " " => {
                timer.toggle();
                true
            }
            "s" | "S" if timer.running.get_untracked() => {
                timer.finish(false);
                true
            }
            "1" => {
                timer.set_mode(Mode::Pomodoro);
                true
            }
            "2" => {
                timer.set_mode(Mode::Short);
                true
            }
            "3" => {
                timer.set_mode(Mode::Long);
                true
            }
            "n" | "N" => {
                timer.tasks.update(|t| t.hidden = false);
                task_editing.set(Some(None));
                true
            }
            "p" | "P" if pip_ok => {
                pip::open(timer);
                true
            }
            "r" | "R" => {
                show_stats.set(true);
                true
            }
            "o" | "O" => {
                show_settings.set(true);
                true
            }
            "?" => {
                show_help.set(true);
                true
            }
            _ => false,
        };
        if handled {
            ev.prevent_default();
        }
    });

    Effect::new(move |_| timer.tasks.with(|t| t.save()));

    // Persist settings, and apply new durations right away if the timer is idle.
    Effect::new(move |_| {
        let s = timer.settings.get();
        s.save();
        if !timer.running.get_untracked() {
            timer.remaining_ms.set(s.duration_ms(timer.mode.get_untracked()));
        }
    });

    Effect::new(move |_| {
        let theme = timer.settings.with(|s| s.theme);
        if let Some(root) = document().document_element() {
            let _ = match theme {
                Theme::Auto => root.remove_attribute("data-theme"),
                t => root.set_attribute("data-theme", t.id()),
            };
        }
    });

    // Background noise only plays while a pomodoro is running.
    Effect::new(move |_| {
        let (noise, volume) = timer.settings.with(|s| (s.noise, s.noise_volume));
        let playing = timer.running.get() && timer.mode.get() == Mode::Pomodoro;
        audio::set_noise(if playing { noise } else { Noise::None }, volume);
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
                    <select
                        class="icon-btn lang-select"
                        aria-label=tr().language
                        on:change=move |ev| {
                            let code = event_target_value(&ev);
                            if let Some(l) = Lang::ALL.into_iter().find(|l| l.code() == code) {
                                let _ = window().location().set_href(l.href());
                            }
                        }
                    >
                        {Lang::ALL.into_iter().map(|l| view! {
                            <option value=l.code() selected=l == i18n::lang()>{l.native_name()}</option>
                        }).collect_view()}
                    </select>
                    <button class="icon-btn" aria-label=tr().stats title=format!("{} (R)", tr().stats) on:click=move |_| show_stats.set(true)>
                        "📊"
                    </button>
                    {pip_ok.then(|| view! {
                        <button class="icon-btn" aria-label=tr().mini_timer title=tr().mini_timer_title on:click=move |_| pip::open(timer)>
                            "⧉"
                        </button>
                    })}
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
                        <button class="skip" title=tr().skip aria-label=tr().skip on:click=move |_| timer.finish(false)>
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

            <tasks::TaskSection timer=timer editing=task_editing />

            <Show when=move || show_settings.get()>
                <SettingsPanel timer=timer on_close=move || show_settings.set(false) />
            </Show>
            <Show when=move || show_stats.get()>
                <stats::StatsDialog on_close=move || show_stats.set(false) />
            </Show>
            <Show when=move || show_help.get()>
                <ShortcutsDialog pip_ok=pip_ok on_close=move || show_help.set(false) />
            </Show>
        </div>
    }
}

#[component]
fn ShortcutsDialog(pip_ok: bool, on_close: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let mut rows = vec![
        (tr().key_space, tr().sc_start),
        ("S", tr().sc_skip),
        ("1 / 2 / 3", tr().sc_modes),
        ("N", tr().sc_new_task),
    ];
    if pip_ok {
        rows.push(("P", tr().sc_mini));
    }
    rows.extend([("R", tr().sc_stats), ("O", tr().sc_settings), ("?", tr().sc_help), ("Esc", tr().sc_close)]);

    view! {
        <div class="overlay" on:click=move |_| on_close()>
            <section class="panel" role="dialog" aria-label=tr().shortcuts on:click=|ev| ev.stop_propagation()>
                <div class="panel-head">
                    <h2>{tr().shortcuts}</h2>
                    <button class="close" aria-label=tr().close on:click=move |_| on_close()>"✕"</button>
                </div>
                <dl class="shortcuts">
                    {rows.into_iter().map(|(k, d)| view! { <dt><kbd>{k}</kbd></dt><dd>{d}</dd> }).collect_view()}
                </dl>
            </section>
        </div>
    }
}

/// Root URL of the current page (language kept), for the OBS overlay link.
fn page_url() -> String {
    let loc = window().location();
    format!("{}{}", loc.origin().unwrap_or_default(), loc.pathname().unwrap_or_default())
}

fn copy_to_clipboard(text: &str) {
    let nav = window().navigator();
    if let Ok(clip) = js_sys::Reflect::get(&nav, &"clipboard".into()) {
        if let Ok(write) = js_sys::Reflect::get(&clip, &"writeText".into()) {
            if let Ok(f) = write.dyn_into::<js_sys::Function>() {
                let _ = f.call1(&clip, &text.into());
            }
        }
    }
}

#[component]
fn SettingsPanel(timer: Timer, on_close: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let s = timer.settings;
    let perm = RwSignal::new(notify::permission());
    let obs_autostart = RwSignal::new(true);
    let obs_sound = RwSignal::new(false);
    let obs_label = RwSignal::new(String::new());
    let copied = RwSignal::new(false);

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

    let range = move |label: &'static str, get: fn(&Settings) -> u32, set: fn(&mut Settings, u32)| {
        view! {
            <label class="field range">
                <span>{label}<output>{move || format!("{} %", s.with(|s| get(s)))}</output></span>
                <input
                    type="range"
                    min="0"
                    max="100"
                    step="5"
                    prop:value=move || s.with(|s| get(s)).to_string()
                    on:input=move |ev| {
                        if let Ok(v) = event_target_value(&ev).parse::<u32>() {
                            s.update(|s| set(s, v.min(100)));
                        }
                    }
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

    let alarm_label = |a: Alarm| match a {
        Alarm::None => tr().alarm_none,
        Alarm::Beep => tr().alarm_beep,
        Alarm::Bell => tr().alarm_bell,
        Alarm::Digital => tr().alarm_digital,
        Alarm::Soft => tr().alarm_soft,
    };
    let noise_label = |n: Noise| match n {
        Noise::None => tr().noise_none,
        Noise::White => tr().noise_white,
        Noise::Pink => tr().noise_pink,
        Noise::Brown => tr().noise_brown,
    };

    let obs_url = move || {
        let st = s.get();
        let mut url = format!(
            "{}?overlay=1&pomo={}&short={}&long={}&interval={}",
            page_url(),
            st.pomodoro_min,
            st.short_min,
            st.long_min,
            st.long_interval
        );
        if obs_autostart.get() {
            url += "&autostart=1";
        }
        if obs_sound.get() {
            url += "&sound=1";
        }
        let label = obs_label.get();
        if !label.trim().is_empty() {
            url += &format!("&label={}", String::from(js_sys::encode_uri_component(label.trim())));
        }
        url
    };

    let add_webhook = move |_| {
        s.update(|s| {
            let id = s.webhooks.iter().map(|w| w.id).max().unwrap_or(0) + 1;
            s.webhooks.push(Webhook { id, ..Webhook::default() });
        })
    };

    view! {
        <div class="overlay" on:click=move |_| on_close()>
            <section class="panel" role="dialog" aria-label=tr().settings.trim_start_matches("⚙ ") on:click=|ev| ev.stop_propagation()>
                <div class="panel-head">
                    <h2>{tr().settings.trim_start_matches("⚙ ")}</h2>
                    <button class="close" aria-label=tr().close on:click=move |_| on_close()>"✕"</button>
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

                <h3>{tr().sound_section}</h3>
                <div class="row">
                    <label class="field grow">
                        <span>{tr().alarm_sound}</span>
                        <select
                            prop:value=move || s.with(|s| s.alarm.id())
                            on:change=move |ev| s.update(|s| s.alarm = Alarm::from_id(&event_target_value(&ev)))
                        >
                            {Alarm::ALL.into_iter().map(|a| view! { <option value=a.id()>{alarm_label(a)}</option> }).collect_view()}
                        </select>
                    </label>
                    <button
                        class="add"
                        disabled=move || s.with(|s| s.alarm == Alarm::None)
                        on:click=move |_| s.with_untracked(|s| audio::play_alarm(s.alarm, s.volume))
                    >
                        {tr().listen}
                    </button>
                </div>
                {range(tr().volume, |s| s.volume, |s, v| s.volume = v)}
                {check(tr().tick, |s| s.tick, |s, v| s.tick = v)}
                <label class="field wide">
                    <span>{tr().noise}</span>
                    <select
                        prop:value=move || s.with(|s| s.noise.id())
                        on:change=move |ev| {
                            audio::unlock();
                            s.update(|s| s.noise = Noise::from_id(&event_target_value(&ev)))
                        }
                    >
                        {Noise::ALL.into_iter().map(|n| view! { <option value=n.id()>{noise_label(n)}</option> }).collect_view()}
                    </select>
                </label>
                {range(tr().noise_volume, |s| s.noise_volume, |s, v| s.noise_volume = v)}

                <h3>{tr().appearance}</h3>
                <label class="field wide">
                    <span>{tr().theme}</span>
                    <select
                        prop:value=move || s.with(|s| s.theme.id())
                        on:change=move |ev| {
                            let id = event_target_value(&ev);
                            s.update(|s| s.theme = Theme::ALL.into_iter().find(|t| t.id() == id).unwrap_or_default())
                        }
                    >
                        {Theme::ALL.into_iter().map(|t| view! { <option value=t.id()>{t.label()}</option> }).collect_view()}
                    </select>
                </label>
                <p class="hint">{tr().shortcuts_hint}</p>

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

                <h3>{tr().obs_section}</h3>
                <label class="toggle">
                    <span>{tr().obs_autostart}</span>
                    <input type="checkbox" prop:checked=move || obs_autostart.get() on:change=move |ev| obs_autostart.set(event_target_checked(&ev)) />
                </label>
                <label class="toggle">
                    <span>{tr().obs_sound}</span>
                    <input type="checkbox" prop:checked=move || obs_sound.get() on:change=move |ev| obs_sound.set(event_target_checked(&ev)) />
                </label>
                <label class="field wide">
                    <span>{tr().obs_label}</span>
                    <input
                        type="text"
                        placeholder=tr().obs_label_placeholder
                        prop:value=move || obs_label.get()
                        on:input=move |ev| obs_label.set(event_target_value(&ev))
                    />
                </label>
                <div class="row">
                    <input class="text-input mono" type="text" readonly aria-label=tr().obs_section prop:value=obs_url />
                    <button
                        class="test"
                        on:click=move |_| {
                            copy_to_clipboard(&obs_url());
                            copied.set(true);
                            set_timeout(move || copied.set(false), Duration::from_secs(2));
                        }
                    >
                        {move || if copied.get() { tr().copied } else { tr().copy }}
                    </button>
                </div>
                <p class="hint">{tr().obs_hint}</p>

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
                    aria-label=tr().webhooks
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
                <button class="close" title=tr().delete aria-label=tr().delete on:click=move |_| s.update(|s| s.webhooks.retain(|w| w.id != id))>
                    "🗑"
                </button>
            </div>
            <input
                class="hook-url"
                type="password"
                aria-label="URL"
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
    let root: web_sys::HtmlElement = root.unchecked_into();

    match overlay_config() {
        Some(cfg) => {
            if let Some(html) = document().document_element() {
                html.class_list().add_1("obs-mode").ok();
            }
            leptos::mount::mount_to(root, move || view! { <OverlayApp cfg=cfg.clone() /> }).forget();
        }
        None => leptos::mount::mount_to(root, App).forget(),
    }
}
