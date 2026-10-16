//! Task list: estimated vs. done pomodoros, an active task that collects
//! finished pomodoros, templates and a Todoist import. Everything is stored
//! in localStorage.

use crate::i18n::{lang, tr, Lang};
use crate::webhook::js_err;
use crate::{now, storage, Mode, Timer};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

const TASKS_KEY: &str = "pomo.tasks";
const TEMPLATES_KEY: &str = "pomo.templates";
const TODOIST_KEY: &str = "pomo.todoist";
const DEFAULT_TODOIST_FILTER: &str = "today | overdue";

#[derive(Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Task {
    pub id: u32,
    pub title: String,
    pub est: u32,
    pub act: u32,
    pub done: bool,
    pub note: String,
    /// Set for imported tasks so a second import skips them.
    pub todoist_id: Option<String>,
}

#[derive(Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskState {
    pub tasks: Vec<Task>,
    pub active: Option<u32>,
    pub hidden: bool,
}

impl TaskState {
    pub fn load() -> Self {
        load_json(TASKS_KEY)
    }

    pub fn save(&self) {
        save_json(TASKS_KEY, self);
    }

    fn next_id(&self) -> u32 {
        self.tasks.iter().map(|t| t.id).max().unwrap_or(0) + 1
    }

    fn find(&self, id: u32) -> Option<&Task> {
        self.tasks.iter().find(|t| t.id == id)
    }

    fn find_mut(&mut self, id: u32) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|t| t.id == id)
    }

    pub fn active_task(&self) -> Option<&Task> {
        self.active.and_then(|id| self.find(id))
    }

    pub fn on_pomodoro_done(&mut self) {
        if let Some(id) = self.active {
            if let Some(t) = self.find_mut(id) {
                t.act += 1;
            }
        }
    }

    /// Drop the active task if it no longer exists, and pick the first open
    /// task when nothing is active.
    fn fix_active(&mut self) {
        if self.active.is_some_and(|id| self.find(id).is_none()) {
            self.active = None;
        }
        if self.active.is_none() {
            self.active = self.tasks.iter().find(|t| !t.done).map(|t| t.id);
        }
    }

    fn push(&mut self, title: String, est: u32, note: String, todoist_id: Option<String>) {
        let id = self.next_id();
        self.tasks.push(Task { id, title, est: est.max(1), note, todoist_id, ..Task::default() });
    }
}

#[derive(Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
struct Template {
    name: String,
    tasks: Vec<TemplateTask>,
}

#[derive(Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
struct TemplateTask {
    title: String,
    est: u32,
    note: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct TodoistPrefs {
    token: String,
    filter: String,
    remember: bool,
}

impl Default for TodoistPrefs {
    fn default() -> Self {
        Self { token: String::new(), filter: DEFAULT_TODOIST_FILTER.into(), remember: true }
    }
}

fn load_json<T: DeserializeOwned + Default>(key: &str) -> T {
    storage()
        .and_then(|s| s.get_item(key).ok().flatten())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_json<T: Serialize>(key: &str, value: &T) {
    if let (Some(s), Ok(raw)) = (storage(), serde_json::to_string(value)) {
        let _ = s.set_item(key, &raw);
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Dialog {
    Templates,
    Todoist,
}

/// `None`: no editor open. `Some(None)`: adding a task. `Some(Some(id))`: editing `id`.
type Editing = RwSignal<Option<Option<u32>>>;

#[component]
pub fn TaskSection(timer: Timer) -> impl IntoView {
    let st = timer.tasks;
    let menu_open = RwSignal::new(false);
    let editing: Editing = RwSignal::new(None);
    let dialog = RwSignal::new(None::<Dialog>);

    let menu_action = move |f: fn(&mut TaskState)| {
        move |_| {
            st.update(|s| {
                f(s);
                s.fix_active();
            });
            menu_open.set(false);
        }
    };
    let open_dialog = move |d: Dialog| {
        move |_| {
            dialog.set(Some(d));
            menu_open.set(false);
        }
    };
    let clear_all = move |_| {
        menu_open.set(false);
        if window().confirm_with_message(tr().confirm_clear_all).unwrap_or(false) {
            st.update(|s| {
                s.tasks.clear();
                s.active = None;
            });
            editing.set(None);
        }
    };

    view! {
        <section class="tasks">
            <div class="tasks-head">
                <h2>{tr().tasks}</h2>
                <div class="menu-wrap">
                    <button
                        class="icon-btn kebab"
                        aria-label=tr().task_menu
                        aria-haspopup="menu"
                        aria-expanded=move || menu_open.get().to_string()
                        on:click=move |_| menu_open.update(|v| *v = !*v)
                    >
                        "⋮"
                    </button>
                    <Show when=move || menu_open.get()>
                        <div class="menu-backdrop" on:click=move |_| menu_open.set(false)></div>
                        <div class="menu" role="menu">
                            <button role="menuitem" on:click=menu_action(|s| s.tasks.retain(|t| !t.done))>
                                <span class="mi">"🗑"</span>{tr().clear_finished}
                            </button>
                            <button role="menuitem" on:click=open_dialog(Dialog::Templates)>
                                <span class="mi">"📋"</span>{tr().use_template}
                            </button>
                            <button role="menuitem" on:click=open_dialog(Dialog::Todoist)>
                                <span class="mi">"☰"</span>{tr().import_todoist}
                            </button>
                            <button role="menuitem" on:click=menu_action(|s| s.tasks.iter_mut().for_each(|t| t.act = 0))>
                                <span class="mi">"✓"</span>{tr().clear_act}
                            </button>
                            <button role="menuitem" on:click=menu_action(|s| s.hidden = !s.hidden)>
                                <span class="mi">"🗃"</span>
                                {move || if st.with(|s| s.hidden) { tr().show_tasks } else { tr().hide_tasks }}
                            </button>
                            <hr />
                            <button role="menuitem" on:click=clear_all>
                                <span class="mi">"🗑"</span>{tr().clear_all}
                            </button>
                        </div>
                    </Show>
                </div>
            </div>

            <Show when=move || !st.with(|s| s.hidden)>
                <For
                    each=move || st.with(|s| s.tasks.iter().map(|t| t.id).collect::<Vec<_>>())
                    key=|id| *id
                    children=move |id| view! { <TaskRow timer=timer id=id editing=editing /> }
                />
                <Show
                    when=move || editing.get() == Some(None)
                    fallback=move || view! {
                        <button class="add-task" on:click=move |_| editing.set(Some(None))>
                            <span class="plus">"+"</span>{tr().add_task}
                        </button>
                    }
                >
                    <TaskEditor timer=timer id=None editing=editing />
                </Show>
                <Summary timer=timer />
            </Show>
        </section>

        <Show when=move || dialog.get() == Some(Dialog::Templates)>
            <TemplatesDialog timer=timer on_close=move || dialog.set(None) />
        </Show>
        <Show when=move || dialog.get() == Some(Dialog::Todoist)>
            <TodoistDialog timer=timer on_close=move || dialog.set(None) />
        </Show>
    }
}

#[component]
fn TaskRow(timer: Timer, id: u32, editing: Editing) -> impl IntoView {
    let st = timer.tasks;
    move || {
        if editing.get() == Some(Some(id)) {
            return view! { <TaskEditor timer=timer id=Some(id) editing=editing /> }.into_any();
        }
        let Some(t) = st.with(|s| s.find(id).cloned()) else { return ().into_any() };
        let active = st.with(|s| s.active == Some(id));
        view! {
            <div class="task" class:active=active class:done=t.done on:click=move |_| st.update(|s| s.active = Some(id))>
                <button
                    class="check"
                    class:checked=t.done
                    aria-label=tr().mark_done
                    aria-pressed=t.done.to_string()
                    on:click=move |ev| {
                        ev.stop_propagation();
                        st.update(|s| if let Some(t) = s.find_mut(id) { t.done = !t.done });
                    }
                >
                    "✓"
                </button>
                <div class="task-main">
                    <span class="task-title">{t.title.clone()}</span>
                    {(!t.note.is_empty()).then(|| view! { <p class="task-note">{t.note.clone()}</p> })}
                </div>
                <span class="task-count"><strong>{t.act}</strong>"/"{t.est}</span>
                <button
                    class="task-edit"
                    aria-label=tr().edit_task
                    on:click=move |ev| {
                        ev.stop_propagation();
                        editing.set(Some(Some(id)));
                    }
                >
                    "⋮"
                </button>
            </div>
        }
        .into_any()
    }
}

#[component]
fn TaskEditor(timer: Timer, id: Option<u32>, editing: Editing) -> impl IntoView {
    let st = timer.tasks;
    let existing = id.and_then(|id| st.with_untracked(|s| s.find(id).cloned()));
    let title = RwSignal::new(existing.as_ref().map(|t| t.title.clone()).unwrap_or_default());
    let est = RwSignal::new(existing.as_ref().map_or(1, |t| t.est));
    let act = RwSignal::new(existing.as_ref().map_or(0, |t| t.act));
    let note = RwSignal::new(existing.as_ref().map(|t| t.note.clone()).unwrap_or_default());
    let show_note = RwSignal::new(!note.get_untracked().is_empty());

    let input_ref = NodeRef::<leptos::html::Input>::new();
    Effect::new(move |_| {
        if let Some(el) = input_ref.get() {
            let _ = el.focus();
        }
    });

    let save = move || {
        let t = title.get_untracked().trim().to_string();
        if t.is_empty() {
            return;
        }
        st.update(|s| {
            match id.and_then(|id| s.find_mut(id)) {
                Some(task) => {
                    task.title = t;
                    task.est = est.get_untracked().max(1);
                    task.act = act.get_untracked();
                    task.note = note.get_untracked();
                }
                None => s.push(t, est.get_untracked(), note.get_untracked(), None),
            }
            s.fix_active();
        });
        editing.set(None);
    };

    let parse = |ev: &leptos::ev::Event, min: u32| event_target_value(ev).parse::<u32>().ok().map(|v| v.clamp(min, 99));

    view! {
        <form class="task-editor" on:submit=move |ev| { ev.prevent_default(); save() }>
            <input
                class="task-input"
                node_ref=input_ref
                type="text"
                aria-label=tr().task_placeholder
                placeholder=tr().task_placeholder
                prop:value=move || title.get()
                on:input=move |ev| title.set(event_target_value(&ev))
            />
            <span class="est-label">{if existing.is_some() { tr().act_est_pomodoros } else { tr().est_pomodoros }}</span>
            <div class="est-row">
                {existing.is_some().then(|| view! {
                    <input
                        class="est-input"
                        type="number"
                        min="0"
                        max="99"
                        aria-label=tr().act_pomodoros
                        prop:value=move || act.get().to_string()
                        on:input=move |ev| if let Some(v) = parse(&ev, 0) { act.set(v) }
                    />
                    <span class="est-sep">"/"</span>
                })}
                <input
                    class="est-input"
                    type="number"
                    min="1"
                    max="99"
                    aria-label=tr().est_pomodoros
                    prop:value=move || est.get().to_string()
                    on:input=move |ev| if let Some(v) = parse(&ev, 1) { est.set(v) }
                />
                <button type="button" class="est-btn" aria-label="+1" on:click=move |_| est.update(|v| *v = (*v + 1).min(99))>"▲"</button>
                <button type="button" class="est-btn" aria-label="-1" on:click=move |_| est.update(|v| *v = v.saturating_sub(1).max(1))>"▼"</button>
            </div>
            <Show
                when=move || show_note.get()
                fallback=move || view! {
                    <button type="button" class="link-btn" on:click=move |_| show_note.set(true)>{tr().add_note}</button>
                }
            >
                <textarea
                    class="task-note-input"
                    rows="3"
                    aria-label=tr().note_placeholder
                    placeholder=tr().note_placeholder
                    prop:value=move || note.get()
                    on:input=move |ev| note.set(event_target_value(&ev))
                />
            </Show>
            <div class="editor-actions">
                {id.map(|id| view! {
                    <button
                        type="button"
                        class="link-btn danger"
                        on:click=move |_| {
                            st.update(|s| {
                                s.tasks.retain(|t| t.id != id);
                                s.fix_active();
                            });
                            editing.set(None);
                        }
                    >
                        {tr().delete}
                    </button>
                })}
                <span class="spacer"></span>
                <button type="button" class="btn-plain" on:click=move |_| editing.set(None)>{tr().cancel}</button>
                <button type="submit" class="btn-dark" disabled=move || title.get().trim().is_empty()>{tr().save}</button>
            </div>
        </form>
    }
}

/// "Pomos: 2/8 · Finish at 16:40 (2.9 h)", like pomofocus.
#[component]
fn Summary(timer: Timer) -> impl IntoView {
    let text = move || {
        let s = timer.settings.get();
        let (act, est, remaining) = timer.tasks.with(|st| {
            let act: u32 = st.tasks.iter().map(|t| t.act).sum();
            let est: u32 = st.tasks.iter().map(|t| t.est).sum();
            let remaining: u32 = st.tasks.iter().filter(|t| !t.done).map(|t| t.est.saturating_sub(t.act)).sum();
            (act, est, remaining)
        });
        if est == 0 {
            return None;
        }
        let mut line = format!("{} {act}/{est}", tr().summary_pomos);
        if remaining > 0 {
            let done = timer.completed.get();
            let interval = s.long_interval.max(1);
            let breaks: u32 = (1..remaining)
                .map(|i| if (done + i) % interval == 0 { s.long_min } else { s.short_min })
                .sum();
            let minutes = remaining * s.pomodoro_min + breaks;
            let end = js_sys::Date::new(&(now() + minutes as f64 * 60_000.0).into());
            let hours = format!("{:.1}", minutes as f64 / 60.0);
            let hours = if lang() == Lang::Fr { format!("{} h", hours.replace('.', ",")) } else { format!("{hours}h") };
            line += &format!(
                " · {} {:02}:{:02} ({hours})",
                tr().summary_finish,
                end.get_hours(),
                end.get_minutes()
            );
        }
        Some(line)
    };
    view! { <Show when=move || text().is_some()><p class="summary">{text}</p></Show> }
}

fn dialog_shell(
    title: &'static str,
    on_close: impl Fn() + Copy + Send + Sync + 'static,
    body: impl IntoView + 'static,
) -> impl IntoView {
    view! {
        <div class="overlay" on:click=move |_| on_close()>
            <section class="panel" role="dialog" aria-label=title on:click=|ev| ev.stop_propagation()>
                <div class="panel-head">
                    <h2>{title}</h2>
                    <button class="close" aria-label=tr().close on:click=move |_| on_close()>"✕"</button>
                </div>
                {body}
            </section>
        </div>
    }
}

#[component]
fn TemplatesDialog(timer: Timer, on_close: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let st = timer.tasks;
    let templates = RwSignal::new(load_json::<Vec<Template>>(TEMPLATES_KEY));
    Effect::new(move |_| templates.with(|t| save_json(TEMPLATES_KEY, t)));
    let name = RwSignal::new(String::new());

    let apply = move |i: usize| {
        let Some(t) = templates.with_untracked(|v| v.get(i).cloned()) else { return };
        st.update(|s| {
            for task in t.tasks {
                s.push(task.title, task.est, task.note, None);
            }
            s.fix_active();
        });
        on_close();
    };
    let save_current = move |_| {
        let n = name.get_untracked().trim().to_string();
        let tasks: Vec<TemplateTask> = st.with_untracked(|s| {
            s.tasks
                .iter()
                .map(|t| TemplateTask { title: t.title.clone(), est: t.est, note: t.note.clone() })
                .collect()
        });
        if n.is_empty() || tasks.is_empty() {
            return;
        }
        templates.update(|v| v.push(Template { name: n, tasks }));
        name.set(String::new());
    };

    let list = move || {
        let items = templates.get();
        if items.is_empty() {
            return view! { <p class="hint">{tr().no_templates}</p> }.into_any();
        }
        items
            .into_iter()
            .enumerate()
            .map(|(i, t)| {
                view! {
                    <div class="tpl-row">
                        <div class="tpl-name">
                            <strong>{t.name}</strong>
                            <span class="hint">{format!(" · {} {}", t.tasks.len(), tr().template_tasks)}</span>
                        </div>
                        <button class="test" on:click=move |_| apply(i)>{tr().add_tasks}</button>
                        <button
                            class="close"
                            aria-label=tr().delete
                            on:click=move |_| templates.update(|v| { v.remove(i); })
                        >
                            "🗑"
                        </button>
                    </div>
                }
            })
            .collect_view()
            .into_any()
    };

    dialog_shell(
        tr().templates_title,
        on_close,
        view! {
            {list}
            <h3>{tr().save_as_template}</h3>
            <div class="row">
                <input
                    class="text-input"
                    type="text"
                    aria-label=tr().template_name
                    placeholder=tr().template_name
                    prop:value=move || name.get()
                    on:input=move |ev| name.set(event_target_value(&ev))
                />
                <button
                    class="test"
                    disabled=move || name.get().trim().is_empty() || st.with(|s| s.tasks.is_empty())
                    on:click=save_current
                >
                    {tr().save}
                </button>
            </div>
        },
    )
}

#[component]
fn TodoistDialog(timer: Timer, on_close: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let st = timer.tasks;
    let prefs: TodoistPrefs = storage()
        .and_then(|s| s.get_item(TODOIST_KEY).ok().flatten())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    let token = RwSignal::new(prefs.token);
    let filter = RwSignal::new(prefs.filter);
    let remember = RwSignal::new(prefs.remember);
    let status = RwSignal::new(String::new());
    let busy = RwSignal::new(false);

    let import = move |_| {
        let tok = token.get_untracked().trim().to_string();
        if tok.is_empty() || busy.get_untracked() {
            return;
        }
        let flt = filter.get_untracked().trim().to_string();
        let keep = remember.get_untracked();
        save_json(
            TODOIST_KEY,
            &TodoistPrefs { token: if keep { tok.clone() } else { String::new() }, filter: flt.clone(), remember: keep },
        );
        busy.set(true);
        status.set(tr().importing.into());
        spawn_local(async move {
            match todoist_fetch(&tok, &flt).await {
                Ok(items) => {
                    let (mut added, mut skipped) = (0, 0);
                    st.update(|s| {
                        for item in items {
                            if s.tasks.iter().any(|t| t.todoist_id.as_deref() == Some(item.id.as_str())) {
                                skipped += 1;
                                continue;
                            }
                            s.push(item.content, 1, item.description, Some(item.id));
                            added += 1;
                        }
                        s.fix_active();
                    });
                    status.set(
                        tr().imported
                            .replace("{n}", &added.to_string())
                            .replace("{skipped}", &skipped.to_string()),
                    );
                }
                Err(e) => status.set(format!("✗ {e}")),
            }
            busy.set(false);
        });
    };

    dialog_shell(
        tr().import_todoist,
        on_close,
        view! {
            <label class="field wide">
                <span>{tr().todoist_token}</span>
                <input
                    type="password"
                    autocomplete="off"
                    prop:value=move || token.get()
                    on:input=move |ev| token.set(event_target_value(&ev))
                />
            </label>
            <p class="hint">{tr().todoist_token_hint}</p>
            <label class="field wide">
                <span>{tr().todoist_filter}</span>
                <input
                    type="text"
                    placeholder=DEFAULT_TODOIST_FILTER
                    prop:value=move || filter.get()
                    on:input=move |ev| filter.set(event_target_value(&ev))
                />
            </label>
            <label class="toggle">
                <span>{tr().remember_token}</span>
                <input
                    type="checkbox"
                    prop:checked=move || remember.get()
                    on:change=move |ev| remember.set(event_target_checked(&ev))
                />
            </label>
            <div class="row">
                <button
                    class="test"
                    disabled=move || busy.get() || token.get().trim().is_empty()
                    on:click=import
                >
                    {tr().import}
                </button>
                <span class="status">{move || status.get()}</span>
            </div>
        },
    )
}

struct TodoistItem {
    id: String,
    content: String,
    description: String,
}

fn parse_item(v: &Value) -> Option<TodoistItem> {
    let id = match v.get("id")? {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    Some(TodoistItem {
        id,
        content: v.get("content")?.as_str()?.to_string(),
        description: v.get("description").and_then(Value::as_str).unwrap_or_default().to_string(),
    })
}

async fn get_json(url: &str, token: &str) -> Result<(u16, Value), String> {
    let init = web_sys::RequestInit::new();
    init.set_method("GET");
    let headers = js_sys::Object::new();
    js_sys::Reflect::set(&headers, &"Authorization".into(), &format!("Bearer {token}").into()).map_err(js_err)?;
    init.set_headers(&headers);

    let resp = JsFuture::from(window().fetch_with_str_and_init(url, &init)).await.map_err(js_err)?;
    let resp: web_sys::Response = resp.dyn_into().map_err(js_err)?;
    let status = resp.status();
    if !resp.ok() {
        return Ok((status, Value::Null));
    }
    let text = JsFuture::from(resp.text().map_err(js_err)?).await.map_err(js_err)?;
    let value = serde_json::from_str(&text.as_string().unwrap_or_default()).map_err(|e| e.to_string())?;
    Ok((status, value))
}

fn check_status(status: u16) -> Result<(), String> {
    match status {
        200..=299 => Ok(()),
        401 | 403 => Err(tr().todoist_bad_token.into()),
        s => Err(format!("HTTP {s}")),
    }
}

/// Todoist API v1 (paginated). Falls back to the older REST v2 endpoint if
/// v1 is not available.
async fn todoist_fetch(token: &str, filter: &str) -> Result<Vec<TodoistItem>, String> {
    let enc = |s: &str| String::from(js_sys::encode_uri_component(s));
    let base = if filter.is_empty() {
        "https://api.todoist.com/api/v1/tasks?limit=200".to_string()
    } else {
        format!("https://api.todoist.com/api/v1/tasks/filter?query={}&limit=200", enc(filter))
    };

    let mut out = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..20 {
        let url = match &cursor {
            Some(c) => format!("{base}&cursor={}", enc(c)),
            None => base.clone(),
        };
        let (status, v) = get_json(&url, token).await?;
        if matches!(status, 404 | 410) && cursor.is_none() {
            return todoist_fetch_v2(token, filter).await;
        }
        check_status(status)?;
        if let Some(items) = v.get("results").and_then(Value::as_array) {
            out.extend(items.iter().filter_map(parse_item));
        }
        cursor = v.get("next_cursor").and_then(Value::as_str).map(String::from);
        if cursor.is_none() {
            break;
        }
    }
    Ok(out)
}

async fn todoist_fetch_v2(token: &str, filter: &str) -> Result<Vec<TodoistItem>, String> {
    let mut url = "https://api.todoist.com/rest/v2/tasks".to_string();
    if !filter.is_empty() {
        url += &format!("?filter={}", String::from(js_sys::encode_uri_component(filter)));
    }
    let (status, v) = get_json(&url, token).await?;
    check_status(status)?;
    Ok(v.as_array().map(|a| a.iter().filter_map(parse_item).collect()).unwrap_or_default())
}

/// Label under the timer: the active task, or a mode-specific prompt.
pub fn focus_label(timer: Timer) -> String {
    timer
        .tasks
        .with(|s| s.active_task().filter(|t| !t.done).map(|t| t.title.clone()))
        .map(|t| format!("{} {t}", tr().working_on))
        .unwrap_or_else(|| {
            if timer.mode.get() == Mode::Pomodoro { tr().focus } else { tr().on_break }.to_string()
        })
}
