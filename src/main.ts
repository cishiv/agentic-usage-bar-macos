import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

type ModelUsage = { name: string; percent: number };

type Usage = {
  session_percent: number;
  session_resets_at: string | null;
  session_severity: string;
  weekly_percent: number;
  weekly_resets_at: string | null;
  weekly_severity: string;
  models: ModelUsage[];
  subscription_type: string | null;
};

type Snapshot = {
  usage: Usage | null;
  error: string | null;
  fetched_at: number | null;
};

let latest: Snapshot | null = null;

const el = (id: string): HTMLElement => {
  const node = document.getElementById(id);
  if (!node) throw new Error(`missing element #${id}`);
  return node;
};

function formatReset(iso: string): string {
  const ms = new Date(iso).getTime() - Date.now();
  if (Number.isNaN(ms)) return "";
  if (ms <= 0) return "resets now";
  const mins = Math.floor(ms / 60_000);
  const days = Math.floor(mins / 1440);
  const hours = Math.floor((mins % 1440) / 60);
  const rem = mins % 60;
  if (days > 0) return `resets in ${days}d ${hours}h`;
  if (hours > 0) return `resets in ${hours}h ${rem}m`;
  return `resets in ${rem}m`;
}

function meter(
  label: string,
  range: string,
  percent: number,
  severity: string,
  resets: string | null,
): string {
  const pct = Math.round(percent);
  return `
    <div class="meter sev-${severity}">
      <div class="meter-head">
        <span class="label">${label}<span class="window"> · ${range}</span></span>
        <span class="pct">${pct}%</span>
      </div>
      <div class="bar"><div class="fill" style="width:${Math.min(pct, 100)}%"></div></div>
      <div class="sub" data-resets="${resets ?? ""}"></div>
    </div>`;
}

function updateCountdowns(): void {
  document.querySelectorAll<HTMLElement>(".sub[data-resets]").forEach((node) => {
    const iso = node.dataset.resets;
    node.textContent = iso ? formatReset(iso) : "";
  });
}

function render(): void {
  if (!latest) return;
  const { usage, error, fetched_at } = latest;
  const stats = el("stats");
  const models = el("models");
  const errorBox = el("error");
  const plan = el("plan");

  plan.textContent = usage?.subscription_type
    ? `${usage.subscription_type.toUpperCase()} plan`
    : "";

  if (error && !usage) {
    stats.innerHTML = "";
    models.innerHTML = "";
    errorBox.hidden = false;
    errorBox.textContent = error;
  } else {
    errorBox.hidden = !error;
    if (error) errorBox.textContent = `Offline · showing last update`;

    if (!usage) {
      stats.innerHTML = `<div class="loading">Loading…</div>`;
      models.innerHTML = "";
    } else {
      stats.innerHTML =
        meter("Session", "5-hour", usage.session_percent, usage.session_severity, usage.session_resets_at) +
        meter("Weekly", "7-day", usage.weekly_percent, usage.weekly_severity, usage.weekly_resets_at);
      models.innerHTML = usage.models.length
        ? `<div class="models-head">This week by model</div>` +
          usage.models
            .map(
              (m) =>
                `<span class="model"><span class="mname">${m.name}</span><span class="mpct">${Math.round(m.percent)}%</span></span>`,
            )
            .join("")
        : "";
    }
  }

  el("updated").textContent = fetched_at
    ? `Updated ${new Date(fetched_at).toLocaleTimeString()}`
    : "—";

  updateCountdowns();
}

function apply(snapshot: Snapshot): void {
  latest = snapshot;
  render();
}

async function init(): Promise<void> {
  el("refresh").addEventListener("click", async () => {
    apply(await invoke<Snapshot>("refresh_usage"));
  });
  el("quit").addEventListener("click", () => {
    void invoke("quit_app");
  });

  // Live updates pushed by the 2-minute background loop.
  await listen<Snapshot>("usage-updated", (event) => apply(event.payload));

  // Refresh whenever the popover opens (window gains focus).
  await getCurrentWindow().onFocusChanged(({ payload: focused }) => {
    if (focused) void invoke<Snapshot>("refresh_usage").then(apply);
  });

  // Tick countdowns every second.
  setInterval(updateCountdowns, 1000);

  // Initial render from whatever is already cached.
  apply(await invoke<Snapshot>("get_usage"));
}

window.addEventListener("DOMContentLoaded", () => {
  void init();
});
