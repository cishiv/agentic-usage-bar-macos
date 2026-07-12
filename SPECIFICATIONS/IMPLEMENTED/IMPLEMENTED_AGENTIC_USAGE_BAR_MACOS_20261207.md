# Agentic Usage Bar (macOS)

> Status: NOT YET IMPLEMENTED
> Date: 2026-07-12
> Author: Shiv

## 1. Overview

Generify the existing Claude-only menubar app into a **multi-provider agentic
usage monitor** for macOS. The app is renamed **`agentic-usage-bar-macos`** and
shows session (5-hour) and weekly (7-day) quota utilization for every supported
provider side by side in the menubar, with a per-provider breakdown in the
popover.

Supported providers in this iteration:

1. **Claude** (Claude Code) — existing implementation, unchanged data source.
2. **Codex** (OpenAI Codex CLI) — new.

The architecture should make adding a third provider a matter of adding one
module + one enum entry, but we do **not** build plugin machinery or config for
hypothetical providers (no premature generalization).

## 2. Goals

1. **Both providers at a glance** — menubar shows session/weekly % per
   provider, color/glyph escalated by the worst severity across all of them.
2. **Per-provider detail on click** — popover shows one section per provider
   with the same breakdown the Claude-only app has today (percentages, reset
   countdowns, per-model rows, plan).
3. **Independent failure** — one provider being logged out / offline /
   uninstalled must not affect the other's display.
4. **Rename** — project, bundle, and all code references move from
   `claude-usage-menubar` to `agentic-usage-bar-macos`.

### Non-goals

- No providers beyond Claude and Codex.
- No provider enable/disable settings UI — a provider that has no local
  credentials is simply omitted.
- No OAuth/token refresh of our own for either provider (we piggyback on each
  CLI's refresh, exactly as today).
- No Windows/Linux support.

## 3. Data Sources

### 3.1 Claude (unchanged)

As per the implemented spec (`IMPLEMENTED_CLAUDE_USAGE_MENUBAR_20262906.md`):
Keychain entry `Claude Code-credentials` →
`GET https://api.anthropic.com/api/oauth/usage` with
`anthropic-beta: oauth-2025-04-20`.

### 3.2 Codex — credentials

Codex CLI stores ChatGPT OAuth credentials in a plain JSON file (no Keychain):

```
~/.codex/auth.json
{
  "auth_mode": "chatgpt",
  "tokens": {
    "id_token": "...",
    "access_token": "...",     // Bearer token we use
    "refresh_token": "...",
    "account_id": "..."        // sent as chatgpt-account-id header
  },
  "last_refresh": "..."
}
```

Codex CLI refreshes this file in place; we read it on **every** fetch (same
piggyback strategy as the Claude Keychain). If `auth_mode` indicates API-key
auth or the file is missing, treat Codex as "not logged in".

### 3.3 Codex — usage endpoint

```
GET https://chatgpt.com/backend-api/wham/usage
Authorization: Bearer <tokens.access_token>
chatgpt-account-id: <tokens.account_id>
```

This is the same undocumented endpoint Codex CLI itself polls (~every 60s) to
power `/status`. Parse defensively; degrade gracefully on shape changes.

### 3.4 Codex — response shape (observed 2026-07-12, live account)

```jsonc
{
  "plan_type": "prolite",
  "rate_limit": {
    "primary_window":   { "used_percent": 8,  "limit_window_seconds": 18000,
                          "reset_after_seconds": 16734, "reset_at": 1783887076 },
    "secondary_window": { "used_percent": 11, "limit_window_seconds": 604800,
                          "reset_after_seconds": 489435, "reset_at": 1784359777 }
  },
  "additional_rate_limits": [
    { "limit_name": "GPT-5.3-Codex-Spark",
      "rate_limit": { "primary_window": { ... }, "secondary_window": { ... } } }
  ],
  "credits": { ... },        // ignored
  "spend_control": { ... }   // ignored
}
```

Mapping to the normalized model:

- `rate_limit.primary_window.used_percent` → **Session %** (window = 5h)
- `rate_limit.secondary_window.used_percent` → **Weekly %** (window = 7d)
- `*.reset_at` (unix seconds) → reset countdowns (convert to RFC3339 so the
  frontend handles both providers identically)
- `additional_rate_limits[]` → per-model rows: `limit_name` +
  `rate_limit.secondary_window.used_percent`
- `plan_type` → plan label
- Codex has no server-sent severity — always derive via the numeric
  70/90 thresholds.

## 4. Architecture

### 4.1 Provider abstraction

Keep it functional and minimal — a shared normalized struct, one module per
provider, no traits:

```
src-tauri/src/providers/
├── mod.rs      # ProviderUsage + shared severity helpers + fetch_all()
├── claude.rs   # today's keychain.rs + usage.rs collapsed in
└── codex.rs    # auth.json reader + wham/usage fetch + normalize
```

```rust
pub struct ProviderUsage {
    pub provider: String,          // "claude" | "codex"
    pub session_percent: f64,
    pub session_resets_at: Option<String>,   // RFC3339
    pub session_severity: String,
    pub weekly_percent: f64,
    pub weekly_resets_at: Option<String>,
    pub weekly_severity: String,
    pub models: Vec<ModelUsage>,   // { name, percent }
    pub plan: Option<String>,
}
```

Each provider module exposes:

```rust
pub async fn fetch() -> Result<ProviderUsage, String>
```

`providers::fetch_all()` runs both concurrently (`tokio::join!`) and returns
`Vec<ProviderSnapshot>` where each entry is
`{ provider, usage: Option<ProviderUsage>, error: Option<String> }`. A missing
credential source ("not installed / not logged in") is an error string like any
other — the tray/popover decide how to render it.

The existing `severity_for`, `worst_severity`, and normalize-style unit tests
move to `providers/mod.rs` / the respective provider module.

### 4.2 Everything else (tray, refresh loop, commands, frontend)

Unchanged in structure, updated to iterate over snapshots:

- `UsageSnapshot` becomes
  `{ providers: Vec<ProviderSnapshot>, fetched_at: RFC3339 }`.
- `get_usage` / `refresh_usage` commands keep their names and semantics.
- 120s refresh loop fetches all providers, updates tray, emits
  `usage-updated`.

## 5. UX Specification

### 5.1 Menubar summary

One compact segment per provider that has credentials, `{session}·{weekly}`:

```
◐ C 11·8  X 8·11
```

- `C` = Claude, `X` = Codex.
- Leading glyph reflects the **worst severity across all providers and
  windows**: `◐` normal, `▲` warning, `●` critical (unchanged thresholds:
  warning ≥ 70%, critical ≥ 90%).
- A provider with credentials but a failed fetch renders as `C –·–`; a
  provider with no credentials at all is omitted from the title entirely.
- If **no** provider is available: `◐ –` (popover explains).

### 5.2 Detail popover

Same frameless anchored window, height grows to fit (~280×560 with both
providers). One section per provider, each identical to today's layout:

- Header: provider name + plan (e.g. **Claude · max**, **Codex · prolite**).
- Session bar + % + "resets in …".
- Weekly bar + % + "resets in …".
- Per-model weekly rows (Claude: `weekly_scoped` limits; Codex:
  `additional_rate_limits`).
- A provider in error state shows its section with the error message (e.g.
  "Codex not logged in — run `codex` and sign in.").
- Providers with no credentials are omitted (matching the menubar).
- Shared footer: "Updated {time}" + **Refresh** + **Quit** (one footer for the
  whole popover, not per provider).

### 5.3 Refresh behavior

Unchanged: fetch on launch, every 120s, on manual refresh, cached snapshot on
popover open. All providers refresh together.

## 6. Error Handling & Edge Cases

| Case | Behavior |
| --- | --- |
| `~/.codex/auth.json` missing | Codex omitted from tray + popover. |
| `auth.json` present but `auth_mode` is API-key / no `tokens.access_token` | Treated as not logged in; omitted. |
| Codex token expired (`401`) | Tray `X –·–`; popover: "Codex auth expired — open Codex to refresh." |
| Claude cases | Unchanged from implemented spec. |
| One provider offline/erroring | Other provider renders normally; glyph severity computed only from providers with data. |
| Endpoint shape changed (either) | Defensive optional parsing; render what parses. |

## 7. Renaming

All references move from `claude-usage-menubar` to `agentic-usage-bar-macos`:

- Repo/directory name (manual step: rename local dir + GitHub repo).
- `package.json` → `"name": "agentic-usage-bar-macos"`.
- `src-tauri/Cargo.toml` → package name `agentic-usage-bar` (crate name
  `agentic_usage_bar_lib`).
- `src-tauri/tauri.conf.json` → `productName: "Agentic Usage Bar"`,
  `identifier: "com.shiv.agentic-usage-bar"`, window title.
- HTTP `User-Agent` header → `agentic-usage-bar-macos`.
- `README.md` title/description rewritten for the multi-provider app.
- Popover markup/CSS class names where they say "claude".

> Note: changing the bundle identifier resets any macOS permissions/login-item
> state tied to the old identifier — acceptable, this app has none that matter.

## 8. Privacy

- Both credentials are read locally and sent only to their own vendor's
  endpoint (`api.anthropic.com`, `chatgpt.com`). No cross-provider mixing, no
  telemetry, no third-party calls.

## 9. Testing

Unit tests only, matching the existing style in `usage.rs`:

- Codex `normalize`: real observed JSON → correct session/weekly/model/plan
  mapping, including `reset_at` unix-seconds → RFC3339 conversion.
- Codex normalize with `{}` → zeros, no panic.
- `auth.json` parsing: valid, missing tokens, API-key mode.
- Tray title formatting: both providers, one provider, one in error, none.
- Severity aggregation across multiple providers.

## 10. Acceptance Criteria

1. Menubar shows `C {s}·{w}  X {s}·{w}` reflecting both live endpoints.
2. Glyph escalates using the worst percentage across both providers.
3. Popover shows a Claude section and a Codex section, each with session,
   weekly, per-model rows, reset countdowns, and plan.
4. Deleting/renaming `~/.codex/auth.json` removes Codex from tray and popover
   without affecting Claude (and vice versa for the Keychain entry).
5. All project references renamed to `agentic-usage-bar-macos`; app builds and
   runs under the new name/identifier.
6. `cargo test` passes with the new provider tests.
