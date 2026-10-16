//! Floating mini timer with the Document Picture-in-Picture API (Chrome,
//! Edge, Opera). The PiP window is a separate document, so it is built with
//! plain DOM calls and kept in sync by an effect, instead of mounting Leptos
//! there (Leptos delegates events to the main document).

use crate::i18n::tr;
use crate::{format_time, Mode, Timer};
use leptos::prelude::*;
use leptos::task::spawn_local;
use std::cell::Cell;
use std::rc::Rc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

const CSS: &str = r#"
html, body { margin: 0; height: 100%; }
body {
  font-family: "Arial Rounded MT Bold", "Nunito", system-ui, sans-serif;
  color: #fff;
  background: #ba4949;
  transition: background-color 0.4s;
  display: flex;
  align-items: center;
  justify-content: center;
  user-select: none;
}
body.mode-short { background: #2f7a7e; }
body.mode-long { background: #397097; }
.pip { text-align: center; padding: 8px; }
.mode { font-size: 14px; font-weight: bold; }
.time { font-size: 56px; font-weight: bold; font-variant-numeric: tabular-nums; line-height: 1.1; }
.task { font-size: 13px; max-width: 260px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-height: 1em; }
.ctrl { display: flex; gap: 8px; justify-content: center; margin-top: 8px; }
button {
  font: inherit; font-weight: bold; border: none; border-radius: 4px; cursor: pointer;
  background: #fff; color: #333; padding: 6px 18px; font-size: 15px;
}
button.skip { background: rgba(0, 0, 0, 0.2); color: #fff; padding: 6px 10px; }
"#;

pub fn supported() -> bool {
    js_sys::Reflect::has(&window(), &"documentPictureInPicture".into()).unwrap_or(false)
}

pub fn open(timer: Timer) {
    let Ok(api) = js_sys::Reflect::get(&window(), &"documentPictureInPicture".into()) else { return };
    // Already open: just focus it.
    if let Ok(existing) = js_sys::Reflect::get(&api, &"window".into()) {
        if let Ok(w) = existing.dyn_into::<web_sys::Window>() {
            let _ = w.focus();
            return;
        }
    }
    let Ok(request) = js_sys::Reflect::get(&api, &"requestWindow".into()) else { return };
    let Ok(request) = request.dyn_into::<js_sys::Function>() else { return };
    let opts = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&opts, &"width".into(), &300.into());
    let _ = js_sys::Reflect::set(&opts, &"height".into(), &190.into());
    let Ok(promise) = request.call1(&api, &opts) else { return };
    let Ok(promise) = promise.dyn_into::<js_sys::Promise>() else { return };

    let owner = Owner::current();
    spawn_local(async move {
        let Ok(win) = JsFuture::from(promise).await else { return };
        let win: web_sys::Window = win.unchecked_into();
        match owner {
            Some(o) => o.with(|| build(win, timer)),
            None => build(win, timer),
        }
    });
}

fn build(win: web_sys::Window, timer: Timer) {
    let Some(doc) = win.document() else { return };
    let (Some(head), Some(body)) = (doc.head(), doc.body()) else { return };
    doc.set_title(tr().mini_timer);
    if let Ok(style) = doc.create_element("style") {
        style.set_text_content(Some(CSS));
        let _ = head.append_child(&style);
    }
    body.set_inner_html(
        r#"<div class="pip"><div class="mode"></div><div class="time"></div><div class="task"></div>
           <div class="ctrl"><button class="toggle"></button><button class="skip">⏭</button></div></div>"#,
    );
    let q = |sel: &str| doc.query_selector(sel).ok().flatten();
    let (Some(mode_el), Some(time_el), Some(task_el), Some(toggle), Some(skip)) =
        (q(".mode"), q(".time"), q(".task"), q(".toggle"), q(".skip"))
    else {
        return;
    };
    skip.set_attribute("title", tr().skip).ok();
    skip.set_attribute("aria-label", tr().skip).ok();

    let on_toggle = Closure::<dyn FnMut()>::new(move || timer.toggle());
    toggle.unchecked_ref::<web_sys::HtmlElement>().set_onclick(Some(on_toggle.as_ref().unchecked_ref()));
    on_toggle.forget();
    let on_skip = Closure::<dyn FnMut()>::new(move || {
        if timer.running.get_untracked() {
            timer.finish(false)
        }
    });
    skip.unchecked_ref::<web_sys::HtmlElement>().set_onclick(Some(on_skip.as_ref().unchecked_ref()));
    on_skip.forget();
    let on_key = Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(move |ev: web_sys::KeyboardEvent| {
        if ev.key() == " " {
            ev.prevent_default();
            timer.toggle();
        }
    });
    let _ = win.add_event_listener_with_callback("keydown", on_key.as_ref().unchecked_ref());
    on_key.forget();

    let closed = Rc::new(Cell::new(false));
    let on_close = {
        let closed = closed.clone();
        Closure::<dyn FnMut()>::new(move || closed.set(true))
    };
    let _ = win.add_event_listener_with_callback("pagehide", on_close.as_ref().unchecked_ref());
    on_close.forget();

    let body_el = body.clone();
    Effect::new(move |_| {
        let remaining = timer.remaining_ms.get();
        let mode = timer.mode.get();
        let running = timer.running.get();
        let task = timer.tasks.with(|s| s.active_task().filter(|t| !t.done).map(|t| t.title.clone()));
        if closed.get() {
            return;
        }
        time_el.set_text_content(Some(&format_time(remaining)));
        mode_el.set_text_content(Some(mode.label()));
        task_el.set_text_content(Some(&task.unwrap_or_default()));
        toggle.set_text_content(Some(if running { "PAUSE" } else { "START" }));
        body_el.set_class_name(match mode {
            Mode::Pomodoro => "mode-pomodoro",
            Mode::Short => "mode-short",
            Mode::Long => "mode-long",
        });
    });
}
