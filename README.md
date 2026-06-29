<div align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="104" alt="Claude Usage icon" />
  <h1>Claude Usage Menubar</h1>
  <p><strong>Your Claude Code session &amp; weekly limits, live in the macOS menubar.</strong></p>
  <p>
    <img alt="platform" src="https://img.shields.io/badge/platform-macOS-111?logo=apple" />
    <img alt="built with Tauri" src="https://img.shields.io/badge/built%20with-Tauri%20v2-24C8DB?logo=tauri" />
    <img alt="Rust" src="https://img.shields.io/badge/Rust-D97757?logo=rust&logoColor=white" />
  </p>
</div>

---

A tiny menubar app that keeps your Claude Code usage in front of you at all
times, so you never get surprised by a rate limit mid-flow.

<div align="center">
  <br/>
  <img src="docs/menubar.png" alt="Always-visible menubar summary" height="26" />
  <br/><sub><em>Always visible in the menubar — colored by severity</em></sub>
  <br/><br/>
  <img src="docs/popover.png" alt="Detail popover" width="300" />
  <br/><sub><em>Click for the full breakdown</em></sub>
  <br/>
</div>

## Features

- **Always-visible summary** — session (5-hour) and weekly (7-day) utilization
  rendered right in the menubar, with a gauge icon that turns
  **green → amber → red** as you approach a limit.
- **Click for detail** — a popover with progress meters, reset countdowns,
  per-model weekly usage, your plan, and the last-fetched time.
- **Self-updating** — refreshes automatically every 2 minutes (and the moment
  you open the popover). A manual **Refresh** button is there too.
- **Menubar-only** — no Dock icon, no window clutter.

## How it works

Claude Code stores its OAuth credentials in the macOS **Keychain**
(`Claude Code-credentials`). This app reads that token locally and calls
Anthropic's own usage endpoint:

```
GET https://api.anthropic.com/api/oauth/usage
```

which returns your live session/weekly utilization and reset times. The token is
read fresh on every poll, so it always uses the credentials Claude Code itself
keeps refreshed — no separate login required.

> ⚠️ The usage endpoint is **undocumented** and may change without notice. If a
> field disappears the app degrades gracefully rather than crashing.

## Privacy

Your credentials never leave your machine except in the `Authorization` header
sent to `api.anthropic.com` (the same place Claude Code already talks to). There
is no telemetry and no third-party network traffic.

## Requirements

- macOS
- [Claude Code](https://claude.com/claude-code) installed and signed in
- For building: [Rust](https://rustup.rs), Xcode command-line tools, and
  [Bun](https://bun.sh)

## Build & run

```bash
bun install

# develop (hot reload)
bun run tauri dev

# build a release .app / .dmg
bun run tauri build
```

The bundled app lands in `src-tauri/target/release/bundle/`.

## Tech

- **[Tauri v2](https://tauri.app)** — Rust host + system WebView
- Rust: `reqwest` (rustls), `serde`, `tokio`, `tauri-plugin-positioner`
- Frontend: Vite + vanilla TypeScript (no framework)

## License

MIT
