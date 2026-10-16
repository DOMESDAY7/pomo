<div align="center">

<img src="icons/icon-192.png" alt="Mascotte tomate" width="120" />

# Pomodoro

**Minuteur Pomodoro gratuit, sans pub et open source, écrit en Rust et compilé en WebAssembly.**

Notifications du navigateur · Webhooks Discord, Slack, ntfy, Gotify · Fonctionne hors ligne · Français / English

[**▶ Essayer sur pomo.domesday.fr**](https://pomo.domesday.fr/) · [**English version**](https://pomo.domesday.fr/en/)

[![Deploy](https://github.com/DOMESDAY7/pomo/actions/workflows/deploy.yml/badge.svg)](https://github.com/DOMESDAY7/pomo/actions/workflows/deploy.yml)
![Rust](https://img.shields.io/badge/Rust-WebAssembly-b7410e?logo=rust)
![Leptos](https://img.shields.io/badge/Leptos-0.8-ef3939)
![PWA](https://img.shields.io/badge/PWA-hors%20ligne-5a0fc8)

<br />

<img src="docs/timer.png" alt="Minuteur Pomodoro" width="49%" />
<img src="docs/break.png" alt="Pause courte" width="49%" />

</div>

---

## Fonctionnalités

| | |
|---|---|
| ⏱ **Durées réglables** | Pomodoro, pause courte, pause longue et fréquence des pauses longues (25 / 5 / 15 min, une pause longue tous les 4 par défaut). |
| 🔁 **Démarrage automatique** | Enchaîne les pauses et les pomodoros sans cliquer, si tu le souhaites. |
| 🔔 **Notifications du navigateur** | Une notification et un signal sonore à la fin de chaque session, même dans l'application installée. |
| 🪝 **Webhooks** | Discord, Slack / Mattermost, ntfy, Gotify, ou n'importe quel service avec un corps de requête personnalisé. Plusieurs webhooks en même temps. |
| 📴 **Hors ligne** | Installable comme une application (PWA). Sans connexion, les webhooks sont ignorés et la notification du navigateur prend le relais. |
| 🔒 **Vie privée** | Aucun compte, aucun pistage, aucun serveur. Les réglages et les URLs de webhook restent dans le navigateur. |
| 🌍 **Français / anglais** | Deux pages, `/` et `/en/`, chacune indexable avec ses balises `hreflang`. L'interface, les messages par défaut et les notifications suivent la langue de la page. |
| ♿ **Accessible** | Contrastes conformes WCAG AA (vérifiés avec axe-core) dans les trois modes. |
| ⚡ **Léger** | Environ 200 Ko de WebAssembly, sans framework JS. |

Le temps est calculé à partir de l'heure de fin prévue : le minuteur reste exact même quand l'onglet est en arrière-plan. Le temps restant s'affiche dans le titre de l'onglet.

<div align="center">
<img src="docs/settings.png" alt="Réglages" width="49%" />
<img src="docs/settings-en.png" alt="Settings" width="49%" />
</div>

## Webhooks

Chaque service attend son propre format. Le minuteur construit la bonne requête selon le type choisi :

| Type | Format envoyé | URL attendue |
|---|---|---|
| **Discord** | `multipart/form-data` avec `payload_json` (`{"content": …}`) | `https://discord.com/api/webhooks/…` |
| **Slack / Mattermost** | `application/x-www-form-urlencoded`, `payload={"text": …}` | `https://hooks.slack.com/services/…` |
| **ntfy** | `text/plain`, titre passé en paramètre `?title=` | `https://ntfy.sh/mon-topic` |
| **Gotify** | `application/x-www-form-urlencoded` (`title`, `message`, `priority`) | `https://gotify.exemple.fr/message?token=…` |
| **Personnalisé** | Méthode, `Content-Type` et corps au choix | n'importe quelle URL |

Les messages sont des modèles avec des variables :

| Variable | Valeur |
|---|---|
| `{count}` | numéro du pomodoro terminé |
| `{mode}` | session qui vient de se terminer |
| `{next}` | session suivante |
| `{min}` | durée de la session suivante, en minutes |
| `{title}`, `{message}` | titre et message rendus (corps personnalisé uniquement) |

Dans un corps personnalisé, les valeurs sont échappées automatiquement selon le `Content-Type` (JSON ou form-urlencoded).

> [!NOTE]
> Les requêtes partent directement du navigateur, donc la cible doit accepter le CORS. Discord et ntfy l'acceptent. Pour Slack et Gotify, le message est envoyé en mode `no-cors` : il arrive bien, mais le navigateur ne peut pas lire la réponse (« envoyé (réponse non lisible) »). En mode personnalisé, un `Content-Type` autre que `text/plain` ou `form-urlencoded` déclenche un preflight CORS que le serveur doit accepter.

> [!WARNING]
> Une URL de webhook suffit pour poster sur ton salon. Elle reste stockée dans le `localStorage` de ton navigateur. Ne la partage pas et ne la commite pas.

## Développement

Prérequis : [Rust](https://rustup.rs), [trunk](https://trunkrs.dev) et `python3` (pour le hook qui génère la page anglaise).

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk      # ou un binaire précompilé depuis les releases GitHub

trunk serve --open       # serveur de dev avec rechargement automatique
trunk build --release    # build optimisé dans dist/
```

### Structure

```
src/
├── main.rs       # UI Leptos, état du minuteur, réglages
├── i18n.rs       # textes FR / EN, langue lue dans <html lang>
├── webhook.rs    # formats par service, envoi des requêtes
└── notify.rs     # notifications navigateur (via le service worker si présent)
index.html        # point d'entrée trunk + contenu statique FR, méta SEO, JSON-LD
i18n/en.html      # blocs anglais qui remplacent les zones <!-- i18n:… --> d'index.html
scripts/build_en.py  # hook post_build (Trunk.toml) qui génère dist/en/index.html
style.css
sw.js             # service worker : cache hors ligne, clic sur notification
manifest.webmanifest
icons/            # icônes PWA générées depuis la mascotte
```

## Déploiement

Le workflow [`.github/workflows/deploy.yml`](.github/workflows/deploy.yml) compile avec trunk et publie sur **GitHub Pages** à chaque push sur `main`.

1. Dans *Settings → Pages*, choisir **GitHub Actions** comme source.
2. Domaine personnalisé : un `CNAME` vers `<user>.github.io` chez le fournisseur DNS, puis renseigner le domaine dans *Settings → Pages* et activer *Enforce HTTPS*.

Le build cible la racine du domaine (`--public-url /`). Pour servir le site sous `https://<user>.github.io/<repo>/`, remplacer par `--public-url /<repo>/`.

## Stack

- [Leptos](https://leptos.dev) 0.8 en mode CSR, réactivité fine sans DOM virtuel
- [wasm-bindgen](https://github.com/rustwasm/wasm-bindgen) / [web-sys](https://docs.rs/web-sys) pour les API du navigateur (Fetch, Notifications, Web Audio, Service Worker)
- [trunk](https://trunkrs.dev) pour le build, avec `wasm-opt -Oz`

### Traductions

Les textes de l'application sont dans `src/i18n.rs`, un `Strings` par langue : le compilateur refuse une traduction incomplète. Le contenu statique (titre, description, FAQ, JSON-LD) est dans `index.html` pour le français et dans `i18n/en.html` pour l'anglais. Une FAQ modifiée doit l'être aux deux endroits de chaque fichier : le texte visible et le JSON-LD.
