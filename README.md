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

## Design notes (v1)

- Providers: Gmail + Outlook via OAuth2 only
- Protocols: IMAP sync + SMTP send (XOAUTH2)
- UI: 3-pane, conversation view, dark-only
- Platforms: macOS + Linux

See [GitHub issues](https://github.com/SalzDevs/harbor/issues) for the v1 roadmap.
