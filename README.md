# Harbor

Fast desktop email client. Rust mail core, Tauri shell, Svelte UI. IMAP/SMTP with a local SQLite cache.

## Workspace

| Path | Crate / package | Role |
|------|-----------------|------|
| `crates/harbor-core` | `harbor-core` | Domain types and mail logic (no GUI, no DB) |
| `crates/harbor-db` | `harbor-db` | SQLite schema and queries |
| `src-tauri` | `harbor-app` | Tauri desktop shell (path is Tauri’s default) |
| `src/` | `harbor` (npm) | Svelte 5 frontend (dark-only) |

## Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) 20+
- Tauri system deps: [macOS / Linux prerequisites](https://v2.tauri.app/start/prerequisites/)

## Run locally

```bash
npm install
npm run desktop
```

Frontend only (browser, no Rust commands):

```bash
npm run dev
```

Rust tests:

```bash
npm run test:rust
# or: cargo test --workspace
```

Production build:

```bash
npm run desktop:build
```

## Packaging

### macOS

```bash
npm run desktop:build
```

Produces `src-tauri/target/release/bundle/macos/Harbor.app` and a `.dmg` installer.
The app bundle goes to `/Applications`. Data lands in `~/Library/Application Support/Harbor/`.

Signing/notarization and auto-update can be staged by setting these env vars before build:

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
export APPLE_ID="you@example.com"
export APPLE_PASSWORD="app-specific-password"
export APPLE_TEAM_ID="TEAMID"
```

### Linux

```bash
npm run desktop:build
```

Produces three artifacts in `src-tauri/target/release/bundle/`:

| Format | Path | Install |
|--------|------|---------|
| AppImage | `appimage/harbor_0.1.0_amd64.AppImage` | `chmod +x` and run, or integrate with AppImageLauncher |
| .deb | `deb/harbor_0.1.0_amd64.deb` | `sudo dpkg -i harbor_0.1.0_amd64.deb` |
| Binary | `harbor-app` | Direct execution |

Data lands in `~/.local/share/harbor/` (or `$XDG_DATA_HOME/harbor`).

Linux requires: `libwebkit2gtk-4.1-0`, `libssl3`. On Debian/Ubuntu:
```bash
sudo apt install libwebkit2gtk-4.1-0 libssl3
```

### CI releases

Tag a push with `v*` to trigger the GitHub Actions release workflow, which builds macOS (arm64 + x64) and Linux (x64) artifacts and creates a draft GitHub release with downloadable installers.

## Data location

SQLite lives in the OS app-data directory:

- macOS: `~/Library/Application Support/Harbor/harbor.sqlite3`
- Linux: `~/.local/share/harbor/harbor.sqlite3` (or `$XDG_DATA_HOME/harbor`)

## OAuth setup

Copy `oauth.json.example` to the data dir as `oauth.json`, or use env vars. Harbor uses loopback redirects (`http://127.0.0.1:<ephemeral>/oauth/callback`).

### Gmail

Google Cloud **Desktop** OAuth client.

```bash
export HARBOR_GMAIL_CLIENT_ID="….apps.googleusercontent.com"
# optional:
export HARBOR_GMAIL_CLIENT_SECRET="…"
```

Scopes: `openid email profile https://mail.google.com/`

### Outlook

Azure app registration (public client / mobile & desktop). Enable allow public client flows. Add redirect `http://127.0.0.1` (any port).

```bash
export HARBOR_OUTLOOK_CLIENT_ID="…"
# optional:
export HARBOR_OUTLOOK_CLIENT_SECRET="…"
```

Scopes: `offline_access openid email profile User.Read` plus Outlook IMAP/SMTP (`IMAP.AccessAsUser.All`, `SMTP.Send`).

## Design notes (v1)

- Providers: Gmail + Outlook via OAuth2 only
- Protocols: IMAP sync + SMTP send (XOAUTH2)
- UI: 3-pane, conversation view, dark-only
- Platforms: macOS + Linux

See [GitHub issues](https://github.com/SalzDevs/harbor/issues) for the v1 roadmap.
