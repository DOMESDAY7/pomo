//! UI strings. The language comes from `<html lang>`: `/` is French and
//! `/en/` is English, each page being its own crawlable URL.

use leptos::prelude::document;
use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Fr,
    En,
}

pub struct Strings {
    pub pomodoro: &'static str,
    pub short_break: &'static str,
    pub long_break: &'static str,
    pub settings: &'static str,
    /// Link to the other language's page, relative to the site root.
    pub switch_label: &'static str,
    pub switch_title: &'static str,
    pub switch_href: &'static str,
    pub offline_banner: &'static str,
    pub skip: &'static str,
    pub focus: &'static str,
    pub on_break: &'static str,

    pub durations: &'static str,
    pub long_interval: &'static str,
    pub auto_breaks: &'static str,
    pub auto_pomodoros: &'static str,
    pub sound: &'static str,
    pub browser_notifications: &'static str,
    pub notify_each_session: &'static str,
    pub test: &'static str,
    pub allow: &'static str,
    pub perm_granted: &'static str,
    pub perm_denied: &'static str,
    pub perm_unsupported: &'static str,
    pub perm_default: &'static str,
    pub offline_hint: &'static str,
    pub webhooks: &'static str,
    pub send_on_pomodoro_end: &'static str,
    pub msg_pomodoro_label: &'static str,
    pub send_on_break_end: &'static str,
    pub msg_break_label: &'static str,
    pub variables_hint: &'static str,
    pub add_webhook: &'static str,
    pub reset: &'static str,

    pub enabled: &'static str,
    pub delete: &'static str,
    pub method: &'static str,
    pub body: &'static str,
    pub cors_hint: &'static str,
    pub send_test: &'static str,
    pub custom: &'static str,

    pub sending: &'static str,
    pub sent: &'static str,
    pub sent_opaque: &'static str,
    pub offline_skipped: &'static str,
    pub offline_no_test: &'static str,
    pub test_title: &'static str,
    pub test_message: &'static str,
    pub notif_test_body: &'static str,
    pub pomodoro_done: &'static str,
    pub break_done: &'static str,
    pub default_msg_pomodoro: &'static str,
    pub default_msg_break: &'static str,

    pub err_scheme: &'static str,
    pub err_unknown: &'static str,
    pub err_cors: &'static str,
}

const FR: Strings = Strings {
    pomodoro: "Pomodoro",
    short_break: "Pause courte",
    long_break: "Pause longue",
    settings: "⚙ Réglages",
    switch_label: "EN",
    switch_title: "English version",
    switch_href: "en/",
    offline_banner: "Hors ligne : seules les notifications du navigateur fonctionnent",
    skip: "Passer",
    focus: "Au travail !",
    on_break: "C'est la pause !",

    durations: "Durées (minutes)",
    long_interval: "Pause longue tous les N pomodoros",
    auto_breaks: "Démarrer les pauses automatiquement",
    auto_pomodoros: "Démarrer les pomodoros automatiquement",
    sound: "Son à la fin",
    browser_notifications: "Notifications du navigateur",
    notify_each_session: "Notifier à la fin de chaque session",
    test: "Tester",
    allow: "Autoriser",
    perm_granted: "✓ Autorisées",
    perm_denied: "✗ Bloquées : autorise-les dans les réglages du site",
    perm_unsupported: "Non supportées par ce navigateur",
    perm_default: "Permission pas encore demandée",
    offline_hint: "Sans connexion, les webhooks sont ignorés et la notification du navigateur est toujours affichée.",
    webhooks: "Webhooks",
    send_on_pomodoro_end: "Envoyer à la fin d'un pomodoro",
    msg_pomodoro_label: "Message fin de pomodoro",
    send_on_break_end: "Envoyer à la fin d'une pause",
    msg_break_label: "Message fin de pause",
    variables_hint: "Variables : {count} {mode} {next} {min}, plus {title} et {message} dans un corps personnalisé. Les URLs restent stockées uniquement dans ce navigateur.",
    add_webhook: "+ Ajouter un webhook",
    reset: "Réinitialiser",

    enabled: "Actif",
    delete: "Supprimer",
    method: "Méthode",
    body: "Corps",
    cors_hint: "Hors text/plain et form-urlencoded, le serveur doit autoriser le CORS depuis cette page.",
    send_test: "Envoyer un test",
    custom: "Personnalisé",

    sending: "Envoi…",
    sent: "✓ envoyé",
    sent_opaque: "✓ envoyé (réponse non lisible)",
    offline_skipped: "Hors ligne : webhooks ignorés, notification navigateur seulement",
    offline_no_test: "Hors ligne : impossible de tester",
    test_title: "🔔 Test Pomodoro",
    test_message: "Test du webhook Pomodoro",
    notif_test_body: "Les notifications fonctionnent !",
    pomodoro_done: "🍅 Pomodoro terminé",
    break_done: "⏰ Pause terminée",
    default_msg_pomodoro: "🍅 Pomodoro #{count} terminé ! Place à la {next} ({min} min).",
    default_msg_break: "⏰ {mode} terminée, retour au travail !",

    err_scheme: "l'URL doit commencer par https://",
    err_unknown: "erreur inconnue",
    err_cors: "le serveur doit autoriser le CORS",
};

const EN: Strings = Strings {
    pomodoro: "Pomodoro",
    short_break: "Short Break",
    long_break: "Long Break",
    settings: "⚙ Settings",
    switch_label: "FR",
    switch_title: "Version française",
    switch_href: "./",
    offline_banner: "Offline: only browser notifications work",
    skip: "Skip",
    focus: "Time to focus!",
    on_break: "Time for a break!",

    durations: "Durations (minutes)",
    long_interval: "Long break every N pomodoros",
    auto_breaks: "Auto-start breaks",
    auto_pomodoros: "Auto-start pomodoros",
    sound: "Sound at the end",
    browser_notifications: "Browser notifications",
    notify_each_session: "Notify at the end of each session",
    test: "Test",
    allow: "Allow",
    perm_granted: "✓ Allowed",
    perm_denied: "✗ Blocked: allow them in the site settings",
    perm_unsupported: "Not supported by this browser",
    perm_default: "Permission not requested yet",
    offline_hint: "When offline, webhooks are skipped and the browser notification is always shown.",
    webhooks: "Webhooks",
    send_on_pomodoro_end: "Send when a pomodoro ends",
    msg_pomodoro_label: "Pomodoro end message",
    send_on_break_end: "Send when a break ends",
    msg_break_label: "Break end message",
    variables_hint: "Variables: {count} {mode} {next} {min}, plus {title} and {message} in a custom body. URLs are only stored in this browser.",
    add_webhook: "+ Add a webhook",
    reset: "Reset",

    enabled: "Enabled",
    delete: "Delete",
    method: "Method",
    body: "Body",
    cors_hint: "Except for text/plain and form-urlencoded, the server must allow CORS from this page.",
    send_test: "Send a test",
    custom: "Custom",

    sending: "Sending…",
    sent: "✓ sent",
    sent_opaque: "✓ sent (response not readable)",
    offline_skipped: "Offline: webhooks skipped, browser notification only",
    offline_no_test: "Offline: can't send a test",
    test_title: "🔔 Pomodoro test",
    test_message: "Pomodoro webhook test",
    notif_test_body: "Notifications are working!",
    pomodoro_done: "🍅 Pomodoro done",
    break_done: "⏰ Break over",
    default_msg_pomodoro: "🍅 Pomodoro #{count} done! Time for a {next} ({min} min).",
    default_msg_break: "⏰ {mode} over, back to work!",

    err_scheme: "the URL must start with https://",
    err_unknown: "unknown error",
    err_cors: "the server must allow CORS",
};

static LANG: OnceLock<Lang> = OnceLock::new();

pub fn lang() -> Lang {
    *LANG.get_or_init(|| {
        let page = document().document_element().map(|e| e.get_attribute("lang").unwrap_or_default());
        if page.is_some_and(|l| l.starts_with("en")) {
            Lang::En
        } else {
            Lang::Fr
        }
    })
}

pub fn tr() -> &'static Strings {
    match lang() {
        Lang::Fr => &FR,
        Lang::En => &EN,
    }
}

/// Messages from older versions saved the French defaults verbatim. Treat
/// them as "default" so the English page shows English messages.
pub fn is_legacy_default(msg: &str) -> bool {
    msg == FR.default_msg_pomodoro || msg == FR.default_msg_break
}
