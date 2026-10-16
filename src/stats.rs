//! Focus history: one entry per finished pomodoro, stored in localStorage.
//! The dialog shows today, the last 7 days, the streak and top tasks, and
//! exports everything as CSV.

use crate::i18n::{hours, lang, tr};
use crate::{now, storage};
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use wasm_bindgen::{JsCast, JsValue};

const LOG_KEY: &str = "pomo.log";
const DAY_MS: f64 = 86_400_000.0;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// End time, ms since epoch.
    pub t: f64,
    /// Length in minutes.
    pub m: u32,
    #[serde(default)]
    pub task: String,
}

fn load() -> Vec<Entry> {
    storage()
        .and_then(|s| s.get_item(LOG_KEY).ok().flatten())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save(log: &[Entry]) {
    if let (Some(s), Ok(raw)) = (storage(), serde_json::to_string(log)) {
        let _ = s.set_item(LOG_KEY, &raw);
    }
}

pub fn record(minutes: u32, task: String) {
    let mut log = load();
    log.push(Entry { t: now(), m: minutes, task });
    save(&log);
}

/// Local calendar day as "YYYY-MM-DD".
fn day_key(ms: f64) -> String {
    let d = js_sys::Date::new(&JsValue::from_f64(ms));
    format!("{:04}-{:02}-{:02}", d.get_full_year(), d.get_month() + 1, d.get_date())
}

fn weekday_label(ms: f64) -> String {
    let d = js_sys::Date::new(&JsValue::from_f64(ms));
    let opts = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&opts, &"weekday".into(), &"short".into());
    String::from(d.to_locale_date_string(lang().code(), &opts))
}

fn local_time(ms: f64) -> String {
    let d = js_sys::Date::new(&JsValue::from_f64(ms));
    format!("{:02}:{:02}", d.get_hours(), d.get_minutes())
}

struct Summary {
    today_count: usize,
    today_min: u32,
    week: Vec<(String, usize, u32)>,
    week_min: u32,
    total_count: usize,
    total_min: u32,
    streak: u32,
    top: Vec<(String, u32)>,
}

fn summarize(log: &[Entry]) -> Summary {
    let t = now();
    let per_day = |key: &str| {
        log.iter()
            .filter(|e| day_key(e.t) == key)
            .fold((0usize, 0u32), |(c, m), e| (c + 1, m + e.m))
    };
    let today = day_key(t);
    let (today_count, today_min) = per_day(&today);

    // Oldest first, today last. Stepping by 24 h is fine for labels; DST
    // shifts never skip a whole calendar day.
    let week: Vec<(String, usize, u32)> = (0..7)
        .rev()
        .map(|i| {
            let ms = t - i as f64 * DAY_MS;
            let (c, m) = per_day(&day_key(ms));
            (weekday_label(ms), c, m)
        })
        .collect();
    let week_min = week.iter().map(|d| d.2).sum();

    // Streak: consecutive days with at least one pomodoro, ending today, or
    // yesterday if today has none yet.
    let days: std::collections::HashSet<String> = log.iter().map(|e| day_key(e.t)).collect();
    let mut streak = 0;
    let mut i = if days.contains(&today) { 0 } else { 1 };
    while days.contains(&day_key(t - i as f64 * DAY_MS)) {
        streak += 1;
        i += 1;
    }

    let mut by_task: Vec<(String, u32)> = Vec::new();
    for e in log.iter().filter(|e| e.t > t - 7.0 * DAY_MS) {
        let name = if e.task.trim().is_empty() { tr().stats_no_task.to_string() } else { e.task.clone() };
        match by_task.iter_mut().find(|(n, _)| *n == name) {
            Some((_, m)) => *m += e.m,
            None => by_task.push((name, e.m)),
        }
    }
    by_task.sort_by(|a, b| b.1.cmp(&a.1));
    by_task.truncate(5);

    Summary {
        today_count,
        today_min,
        week,
        week_min,
        total_count: log.len(),
        total_min: log.iter().map(|e| e.m).sum(),
        streak,
        top: by_task,
    }
}

fn csv(log: &[Entry]) -> String {
    let quote = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
    let mut out = String::from("date,time,minutes,task\n");
    for e in log {
        out += &format!("{},{},{},{}\n", day_key(e.t), local_time(e.t), e.m, quote(&e.task));
    }
    out
}

fn download(name: &str, content: &str) {
    let parts = js_sys::Array::of1(&JsValue::from_str(content));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("text/csv;charset=utf-8");
    let Ok(blob) = web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts) else { return };
    let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else { return };
    if let Ok(a) = document().create_element("a") {
        let a: web_sys::HtmlAnchorElement = a.unchecked_into();
        a.set_href(&url);
        a.set_download(name);
        a.click();
    }
    let _ = web_sys::Url::revoke_object_url(&url);
}

fn fmt_minutes(m: u32) -> String {
    if m < 60 { format!("{m} min") } else { hours(m as f64 / 60.0) }
}

#[component]
pub fn StatsDialog(on_close: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let log = RwSignal::new(load());

    let clear = move |_| {
        if window().confirm_with_message(tr().confirm_clear_stats).unwrap_or(false) {
            save(&[]);
            log.set(Vec::new());
        }
    };
    let export = move |_| log.with_untracked(|l| download(&format!("pomodoro-{}.csv", day_key(now())), &csv(l)));

    let body = move || {
        let entries = log.get();
        if entries.is_empty() {
            return view! { <p class="hint">{tr().no_stats}</p> }.into_any();
        }
        let s = summarize(&entries);
        let max = s.week.iter().map(|d| d.1).max().unwrap_or(1).max(1);
        view! {
            <div class="stat-cards">
                <div class="stat-card">
                    <span class="stat-label">{tr().stats_today}</span>
                    <strong class="stat-value">{s.today_count}</strong>
                    <span class="stat-sub">{fmt_minutes(s.today_min)}</span>
                </div>
                <div class="stat-card">
                    <span class="stat-label">{tr().stats_week}</span>
                    <strong class="stat-value">{s.week.iter().map(|d| d.1).sum::<usize>()}</strong>
                    <span class="stat-sub">{fmt_minutes(s.week_min)}</span>
                </div>
                <div class="stat-card">
                    <span class="stat-label">{tr().stats_streak}</span>
                    <strong class="stat-value">{s.streak}</strong>
                    <span class="stat-sub">{tr().stats_days}</span>
                </div>
                <div class="stat-card">
                    <span class="stat-label">{tr().stats_total}</span>
                    <strong class="stat-value">{s.total_count}</strong>
                    <span class="stat-sub">{fmt_minutes(s.total_min)}</span>
                </div>
            </div>

            <h3>{tr().stats_week}</h3>
            <ul class="bars" aria-label=tr().stats_week>
                {s.week.into_iter().map(|(label, count, minutes)| {
                    let pct = count as f64 / max as f64 * 100.0;
                    let title = format!("{label} : {count} {} · {}", tr().stats_pomodoros, fmt_minutes(minutes));
                    view! {
                        <li class="bar-col" title=title.clone() aria-label=title>
                            <span class="bar-value">{count}</span>
                            <span class="bar-track"><span class="bar" style=format!("height:{pct:.0}%")></span></span>
                            <span class="bar-label">{label}</span>
                        </li>
                    }
                }).collect_view()}
            </ul>

            {(!s.top.is_empty()).then(|| view! {
                <h3>{tr().stats_top_tasks}</h3>
                <ul class="top-tasks">
                    {s.top.into_iter().map(|(name, m)| view! {
                        <li><span class="top-name">{name}</span><span class="top-min">{fmt_minutes(m)}</span></li>
                    }).collect_view()}
                </ul>
            })}
        }
        .into_any()
    };

    view! {
        <div class="overlay" on:click=move |_| on_close()>
            <section class="panel" role="dialog" aria-label=tr().stats on:click=|ev| ev.stop_propagation()>
                <div class="panel-head">
                    <h2>{tr().stats}</h2>
                    <button class="close" aria-label=tr().close on:click=move |_| on_close()>"✕"</button>
                </div>
                {body}
                <div class="row end">
                    <button class="reset" disabled=move || log.with(|l| l.is_empty()) on:click=clear>{tr().clear_stats}</button>
                    <button class="test" disabled=move || log.with(|l| l.is_empty()) on:click=export>{tr().export_csv}</button>
                </div>
            </section>
        </div>
    }
}
