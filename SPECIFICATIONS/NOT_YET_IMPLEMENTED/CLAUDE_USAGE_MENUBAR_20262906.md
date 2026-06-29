# Claude Usage Menubar

> Status: NOT YET IMPLEMENTED
> Date: 2026-06-29
> Author: Shiv

## 1. Overview

A macOS menubar app (built with **Tauri v2**) that surfaces Claude Code usage
limits at a glance. It shows the current **session (5-hour)** and **weekly
(7-day)** quota utilization directly in the menubar, and reveals a detailed
breakdown — including reset countdowns and per-model weekly usage — in a popover
when clicked. It refreshes itself automatically every 2 minutes.

## 2. Goals

1. **Summary always visible** — session % and weekly % rendered as text in the
   macOS menubar, color-coded by severity.
2. **Details on click** — a popover anchored to the menubar showing the full
   breakdown plus a "last fetched" timestamp.
3. **Self-updating** — automatically refreshes every ~2 minutes, with a manual
   refresh control.

### Non-goals

- No Windows/Linux support (macOS menubar + Keychain only).
- No historical charts or cost analytics (this reads live quota, not the JSONL
  transcript logs that tools like `ccusage` parse).
- No OAuth login flow of our own — we piggyback on the credentials Claude Code
  already manages.

## 3. Data Source

### 3.1 Credentials

Claude Code stores its OAuth credentials in the **macOS Keychain**:

- Service: `Claude Code-credentials`
- Account: the macOS username (e.g. `shiv`)
- Value: a JSON blob `{ "claudeAiOauth": { "accessToken", "refreshToken",
  "expiresAt", "scopes", "subscriptionType", "rateLimitTier" } }`

> The token is **also** mirrored to `~/.claude/.credentials.json`, but that copy
> can be stale. The Keychain copy is the live one (Claude Code refreshes it in
> place). We therefore read the Keychain on **every** fetch — this lets us
> piggyback on Claude Code's own token refresh and avoid implementing the OAuth
> refresh dance ourselves.

We read the Keychain via the macOS `security` CLI:
`security find-generic-password -s "Claude Code-credentials" -a "$USER" -w`

### 3.2 Usage endpoint

```
GET https://api.anthropic.com/api/oauth/usage
Authorization: Bearer <accessToken>
anthropic-beta: oauth-2025-04-20
```

This is an **undocumented** endpoint and may change without notice — the app
must degrade gracefully if its shape changes (see Error Handling).

### 3.3 Response shape (observed)

```jsonc
{
  "five_hour":  { "utilization": 11.0, "resets_at": "2026-06-29T13:19:59Z", ... },
  "seven_day":  { "utilization": 8.0,  "resets_at": "2026-07-05T02:59:59Z", ... },
  "seven_day_opus":   null,
  "seven_day_sonnet": { "utilization": 2.0, "resets_at": "...", ... },
  "limits": [
    { "kind": "session",       "group": "session", "percent": 11, "severity": "normal", "resets_at": "...", "is_active": true },
    { "kind": "weekly_all",    "group": "weekly",  "percent": 8,  "severity": "normal", "resets_at": "...", "is_active": false },
    { "kind": "weekly_scoped", "group": "weekly",  "percent": 2,  "severity": "normal", "resets_at": "...",
      "scope": { "model": { "display_name": "Sonnet" } }, "is_active": false }
  ],
  "spend": { ... },          // credit/overage info, may be disabled
  "extra_usage": { ... }
}
```

We primarily use:
- `five_hour.utilization` → **Session %**
- `seven_day.utilization` → **Weekly %**
- `seven_day_opus` / `seven_day_sonnet` → per-model weekly (shown if non-null)
- `*.resets_at` → reset countdowns
- `severity` (from `limits[]`) → color band, with a numeric-threshold fallback.

## 4. Architecture

```
┌─────────────────────────── Tauri app ───────────────────────────┐
│                                                                  │
│  Rust backend (src-tauri)                Webview (frontend)      │
│  ────────────────────────                ──────────────────      │
│  • read Keychain token                                           │
│  • GET /api/oauth/usage  ── reqwest                              │
│  • parse → Usage struct                                          │
│  • update tray title "S 11%  W 8%"                               │
│  • 120s tokio interval ───┐                                      │
│  • emit "usage-updated" ──┴──────────────►  listen → render      │
│  • tray left-click ───────────────────────► show/position popup  │
│  • #[command] get_usage / refresh ◄──────── invoke() on demand   │
│  • ActivationPolicy::Accessory (no Dock icon)                    │
└──────────────────────────────────────────────────────────────────┘
```

### 4.1 Tech stack

- **Tauri v2** (Rust host + system WebView).
- **Rust**: `reqwest` (rustls, JSON), `serde`/`serde_json`, `tokio`,
  `chrono` (reset countdowns), `tauri-plugin-positioner` (anchor popover to
  tray).
- **Frontend**: Vite + **vanilla TypeScript** (no React — keep it simple). Plain
  HTML/CSS for the popover.
- **Package manager**: `bun`.

### 4.2 Rust modules (`src-tauri/src`)

- `keychain.rs` — `read_token() -> Result<String>` via `security` CLI.
- `usage.rs` — `Usage` types + `fetch_usage(token) -> Result<Usage>` + severity
  mapping. Pure parsing/derivation kept in small testable functions.
- `tray.rs` — build tray, format the title string, color/emoji prefix,
  left-click handler that toggles the popover window.
- `lib.rs` / `main.rs` — app setup: accessory policy, spawn the 120s refresh
  loop, register `#[command]`s, wire the positioner plugin.

### 4.3 Tauri commands (invoked from the webview)

- `get_usage() -> UsageSnapshot` — return the most recent fetched snapshot
  (cached in app state) without hitting the network.
- `refresh_usage() -> UsageSnapshot` — force a fetch now, update tray, return it.

`UsageSnapshot` = `{ usage: Usage | null, error: string | null, fetched_at: RFC3339 }`.

## 5. UX Specification

### 5.1 Menubar summary (always visible)

- Text: `◐ S {session}%  W {weekly}%` (e.g. `◐ S 11%  W 8%`).
- Set via the macOS tray **title** (`TrayIcon::set_title`) — text rendered next
  to a small template icon.
- **Color/severity** (driven by the worse of the two, using `severity` when
  present else numeric thresholds):
  - `normal`  (< 70%) → default menubar color.
  - `warning` (70–89%) → yellow/amber indicator.
  - `critical`(≥ 90%) → red indicator.
  - macOS template icons render monochrome, so severity is conveyed by a leading
    glyph (`◐` normal, `▲` warning, `●` red) plus color in the popover.
- While loading / on error: `◐ S –  W –` (and the popover explains the error).

### 5.2 Detail popover (on click)

A small frameless window (~280×340) anchored under the tray icon
(`tauri-plugin-positioner`, `TrayCenter`), dismissed on blur. Contents:

- **Session (5-hour)**: big % + colored progress bar + "resets in 2h 14m".
- **Weekly (7-day)**: big % + colored progress bar + "resets in 5d 14h".
- **Per-model weekly** (only rows that are non-null): e.g. "Sonnet 2%".
- **Plan**: subscription type (e.g. `max`).
- **Footer**: "Updated 11:02:14" (last fetched) + a manual **Refresh** button +
  a **Quit** button.
- Reset countdowns are computed in the frontend from `resets_at` and tick down.

### 5.3 Refresh behavior

- Initial fetch on launch.
- Background fetch every **120 seconds** (tokio interval in Rust); updates the
  tray title and emits `usage-updated` so an open popover re-renders.
- Manual **Refresh** button → `refresh_usage` command.
- Popover open also triggers a `get_usage` to render the latest cached snapshot
  immediately.

## 6. Error Handling & Edge Cases

| Case | Behavior |
| --- | --- |
| Keychain entry missing | Tray `◐ —`; popover: "Claude Code not logged in. Run `claude` and sign in." |
| Token present but `401` | Tray `◐ —`; popover: "Auth expired — open Claude Code to refresh." (We do not refresh tokens ourselves.) |
| Network/offline | Keep last good snapshot; popover shows stale data + "Offline, last updated …". |
| Endpoint shape changed | Parse defensively (all optional); show whatever fields parse, log the rest. |
| Non-macOS | Out of scope; app targets macOS only. |

## 7. Privacy

- Credentials are read locally from the Keychain and used only to call
  Anthropic's own usage endpoint. They never leave the machine except in the
  `Authorization` header to `api.anthropic.com`.
- No telemetry, no analytics, no third-party network calls.

## 8. Project Layout

```
claude-usage-menubar/
├── README.md
├── SPECIFICATIONS/
│   ├── _WORKFLOW.md
│   ├── NOT_YET_IMPLEMENTED/
│   └── IMPLEMENTED/
├── package.json                 # bun + vite + tauri scripts
├── index.html                   # popover markup
├── src/                         # frontend (vanilla TS)
│   ├── main.ts
│   └── styles.css
└── src-tauri/
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── icons/                   # app icon + tray template icon
    └── src/
        ├── main.rs / lib.rs
        ├── keychain.rs
        ├── usage.rs
        └── tray.rs
```

## 9. Build & Run

- Dev: `bun install && bun run tauri dev`
- Release: `bun run tauri build` → produces `.app` / `.dmg`.
- Requires: Rust toolchain, Xcode command-line tools, Tauri CLI.

## 10. Acceptance Criteria

1. Menubar shows `S {n}%  W {n}%` reflecting the live `oauth/usage` response.
2. Color/glyph escalates at the 70% and 90% thresholds.
3. Clicking the menubar opens a popover with session, weekly, per-model weekly,
   reset countdowns, plan, last-fetched time, refresh, and quit.
4. Values refresh automatically every ~2 minutes without interaction.
5. No Dock icon (menubar-only accessory app).
6. Graceful messaging when not logged in / token expired / offline.
