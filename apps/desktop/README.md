# CompanyOS Desktop (Tauri) — Phase 1.11

Native shell around the existing web app (`apps/web`). Does **not** rewrite the
web client in Dart/Rust UI.

## Features

- System tray
- Native notifications (via Tauri notification plugin; mocked in unit tests)
- Global copilot hotkey: `⌥Space` / `Alt+Space`
- Deep links: `companyos://record/{id}` (optional `?org=`)
- Offline shell showing last cached dashboard JSON

## Dev

```bash
# Terminal A — web
pnpm --filter @companyos/web dev

# Terminal B — desktop
cd apps/desktop
npm install
npm run tauri dev
```

## CI

Linux: `cargo test` / `cargo check` in `src-tauri` (no signed macOS/iOS).
Store-signed builds and crash reporting are follow-ups.

## Config

- `COMPANYOS_WEB_URL` — default `http://127.0.0.1:3000`
