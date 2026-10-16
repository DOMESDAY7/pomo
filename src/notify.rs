//! Browser notifications. Goes through the service worker registration when
//! there is one: installed PWAs (notably on Android) reject `new Notification`.

use leptos::prelude::window;
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Granted,
    Denied,
    Default,
    Unsupported,
}

pub fn permission() -> Permission {
    if !js_sys::Reflect::has(&window(), &"Notification".into()).unwrap_or(false) {
        return Permission::Unsupported;
    }
    match web_sys::Notification::permission() {
        web_sys::NotificationPermission::Granted => Permission::Granted,
        web_sys::NotificationPermission::Denied => Permission::Denied,
        _ => Permission::Default,
    }
}

/// Ask for permission. Browsers only show the prompt from a user gesture.
pub fn request(on_done: impl FnOnce(Permission) + 'static) {
    if permission() != Permission::Default {
        on_done(permission());
        return;
    }
    let Ok(promise) = web_sys::Notification::request_permission() else { return };
    spawn_local(async move {
        let _ = JsFuture::from(promise).await;
        on_done(permission());
    });
}

pub fn is_online() -> bool {
    window().navigator().on_line()
}

pub fn show(title: &str, body: &str) {
    if permission() != Permission::Granted {
        return;
    }
    let opts = web_sys::NotificationOptions::new();
    opts.set_body(body);
    opts.set_tag("pomo");
    opts.set_icon("icons/icon-192.png");

    let sw = window().navigator().service_worker();
    if sw.controller().is_some() {
        let title = title.to_string();
        spawn_local(async move {
            let Ok(ready) = sw.ready() else { return };
            let Ok(reg) = JsFuture::from(ready).await else { return };
            let reg: web_sys::ServiceWorkerRegistration = reg.unchecked_into();
            if let Ok(p) = reg.show_notification_with_options(&title, &opts) {
                let _ = JsFuture::from(p).await;
            }
        });
        return;
    }

    if let Ok(n) = web_sys::Notification::new_with_options(title, &opts) {
        let n2 = n.clone();
        let onclick = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            let _ = window().focus();
            n2.close();
        });
        n.set_onclick(Some(onclick.as_ref().unchecked_ref()));
        onclick.forget();
    }
}
