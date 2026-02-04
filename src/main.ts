import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

interface TokenUsage {
  input_tokens: number;
  output_tokens: number;
  cache_creation_input_tokens: number;
  cache_read_input_tokens: number;
}

interface ModelUsage {
  model: string;
  display_name: string;
  tokens: TokenUsage;
  cost_usd: number;
}

interface QuotaInfo {
  messages_in_window: number;
  window_hours: number;
  estimated_limit: number;
  usage_percent: number;
  plan: string;
  week_usage_percent: number;
  week_limit_hours: number;
}

interface ActiveSession {
  session_id: string;
  project: string;
  directory: string;
  first_activity: string;
  last_activity: string;
  duration_minutes: number;
  message_count: number;
  total_tokens: number;
  cost_usd: number;
  model: string;
  model_display_name: string;
  context_remaining_percent: number;
  todo_count: number;
}

interface DailyActivity {
  date: string;
  prompt_count: number;
}

interface WeekDay {
  date: string;
  day_name: string;
  prompt_count: number;
  is_today: boolean;
  is_future: boolean;
}

interface WeeklyUsage {
  days: WeekDay[];
  week_start: string;
  estimated_weekly_limit: number;
}

interface UsageStats {
  total_tokens: TokenUsage;
  total_cost_usd: number;
  by_model: ModelUsage[];
  session_count: number;
  last_updated: string;
  quota: QuotaInfo;
  active_sessions: ActiveSession[];
  daily_activity: DailyActivity[];
  weekly_usage: WeeklyUsage;
}

let transparency = 85;
let settingsOpen = false;
let isRendering = false;
let retryCount = 0;
let retryTimeoutId: ReturnType<typeof setTimeout> | null = null;
// Use sessionStorage to persist reload state across page reloads and prevent infinite loops
let reloadAttempted = sessionStorage.getItem("cc-widget-reload-attempted") === "true";
const MAX_RETRIES = 5;
const BASE_RETRY_DELAY_MS = 1000;

function loadSettings(): void {
  const saved = localStorage.getItem("cc-widget-settings");
  if (saved) {
    const settings = JSON.parse(saved);
    transparency = settings.transparency ?? 85;
  }
  applyTransparency();
}

function saveSettings(): void {
  localStorage.setItem("cc-widget-settings", JSON.stringify({ transparency }));
}

function applyTransparency(): void {
  const container = document.querySelector(".container") as HTMLElement;
  if (container) {
    container.style.background = `rgba(20, 20, 30, ${transparency / 100})`;
  }
}

function formatNumber(num: number): string {
  if (num >= 1_000_000) {
    return (num / 1_000_000).toFixed(1) + "M";
  } else if (num >= 1_000) {
    return (num / 1_000).toFixed(1) + "K";
  }
  return num.toLocaleString();
}

function getModelClass(model: string): string {
  if (model.includes("opus")) return "model-opus";
  if (model.includes("sonnet")) return "model-sonnet";
  if (model.includes("haiku")) return "model-haiku";
  return "";
}

function getQuotaColor(percent: number): string {
  if (percent >= 80) return "#ef4444";
  if (percent >= 50) return "#f59e0b";
  return "#22c55e";
}

function formatDuration(minutes: number): string {
  if (minutes < 1) return "<1m";
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const mins = minutes % 60;
  if (mins === 0) return `${hours}h`;
  return `${hours}h ${mins}m`;
}

function formatDirectory(path: string, maxLength: number = 30): string {
  if (path.length <= maxLength) return path;
  // Truncate from the beginning, keep the end
  return "..." + path.slice(-(maxLength - 3));
}

function getNextWeeklyReset(): string {
  const now = new Date();
  const dayOfWeek = now.getDay(); // 0 = Sunday
  // Reset is on Sunday (day 0)
  const daysUntilReset = dayOfWeek === 0 ? 7 : 7 - dayOfWeek;
  const resetDate = new Date(now);
  resetDate.setDate(now.getDate() + daysUntilReset);
  return `${resetDate.getMonth() + 1}/${resetDate.getDate()}`;
}

function renderActivityHeatmap(dailyActivity: DailyActivity[]): string {
  // Build a map of date -> prompt_count
  const activityMap = new Map<string, number>();
  let maxCount = 0;
  for (const day of dailyActivity) {
    activityMap.set(day.date, day.prompt_count);
    if (day.prompt_count > maxCount) maxCount = day.prompt_count;
  }

  // Generate exactly 12 weeks of dates, ending on Saturday of current week
  // This ensures the grid is always full with the latest week on the far-right
  const today = new Date();
  const todayDow = today.getDay(); // 0=Sun, 6=Sat

  // Find the Saturday of the current week (end of the rightmost column)
  const endDate = new Date(today);
  endDate.setDate(today.getDate() + (6 - todayDow));

  // Go back 12 weeks (84 days) from that Saturday to get the starting Sunday
  const startDate = new Date(endDate);
  startDate.setDate(endDate.getDate() - 83);

  // Helper to format date as YYYY-MM-DD in local timezone
  const formatLocalDate = (date: Date): string => {
    const year = date.getFullYear();
    const month = String(date.getMonth() + 1).padStart(2, "0");
    const day = String(date.getDate()).padStart(2, "0");
    return `${year}-${month}-${day}`;
  };

  const todayStr = formatLocalDate(today);

  // Generate all 84 days (12 complete weeks)
  const days: { date: string; count: number; dayOfWeek: number; isFuture: boolean }[] = [];
  for (let i = 0; i < 84; i++) {
    const d = new Date(startDate);
    d.setDate(startDate.getDate() + i);
    const dateStr = formatLocalDate(d);
    const count = activityMap.get(dateStr) || 0;
    const isFuture = dateStr > todayStr;
    days.push({ date: dateStr, count, dayOfWeek: d.getDay(), isFuture });
  }

  // Group into exactly 12 weeks (7 days each)
  const weeks: { date: string; count: number; dayOfWeek: number; isFuture: boolean }[][] = [];
  for (let w = 0; w < 12; w++) {
    weeks.push(days.slice(w * 7, (w + 1) * 7));
  }

  // Get intensity level (0-4) based on count
  const getLevel = (count: number): number => {
    if (count === 0) return 0;
    if (maxCount === 0) return 0;
    const ratio = count / maxCount;
    if (ratio <= 0.25) return 1;
    if (ratio <= 0.5) return 2;
    if (ratio <= 0.75) return 3;
    return 4;
  };

  // Build grid HTML
  let html = '<div class="heatmap-grid">';

  // For each day of week (row)
  for (let dow = 0; dow < 7; dow++) {
    html += '<div class="heatmap-row">';
    // Day label
    const dayLabels = ["S", "M", "T", "W", "T", "F", "S"];
    if (dow === 1 || dow === 3 || dow === 5) {
      html += `<span class="heatmap-label">${dayLabels[dow]}</span>`;
    } else {
      html += '<span class="heatmap-label"></span>';
    }

    // For each week (column) - weeks are already in order, 12 complete weeks
    for (const week of weeks) {
      const dayData = week[dow]; // Direct index since each week has all 7 days in order
      if (dayData.isFuture) {
        html += '<div class="heatmap-cell future"></div>';
      } else {
        const level = getLevel(dayData.count);
        const tooltip = `${dayData.date}: ${dayData.count}`;
        html += `<div class="heatmap-cell level-${level}" data-tooltip="${tooltip}"></div>`;
      }
    }
    html += "</div>";
  }

  html += "</div>";
  return html;
}

function renderWeeklyUsageChart(weeklyUsage: WeeklyUsage): string {
  const { days, estimated_weekly_limit } = weeklyUsage;

  // Calculate cumulative usage for each day and the max for scaling
  let cumulative = 0;
  const cumulativeData = days.map((day) => {
    cumulative += day.prompt_count;
    return { ...day, cumulative };
  });

  // Find max value for scaling (either cumulative usage or daily pace target)
  const dailyPaceTarget = estimated_weekly_limit / 7;
  const maxCumulative = cumulativeData[cumulativeData.length - 1]?.cumulative || 0;
  const maxValue = Math.max(maxCumulative, estimated_weekly_limit, dailyPaceTarget * 2);

  // Chart dimensions
  const chartHeight = 60;

  // Generate bars and pace line points using percentage positioning
  let barsHtml = "";
  let paceLinePoints = "0,100";
  let labelsHtml = "";

  for (let i = 0; i < days.length; i++) {
    const day = cumulativeData[i];
    const barHeightPercent = maxValue > 0 ? (day.cumulative / maxValue) * 100 : 0;
    const barXPercent = (i / days.length) * 100;
    const barWidthPercent = 100 / days.length - 1; // Leave small gap

    // Pace line point (linear from 0 to estimated_weekly_limit over 7 days)
    const paceYPercent = 100 - ((((i + 1) / 7) * estimated_weekly_limit) / maxValue) * 100;
    const paceXPercent = ((i + 0.5) / days.length) * 100;
    paceLinePoints += ` ${paceXPercent},${paceYPercent}`;

    // Bar styling based on state
    let barClass = "week-bar";
    if (day.is_today) barClass += " today";
    else if (day.is_future) barClass += " future";
    else if (day.cumulative > ((i + 1) / 7) * estimated_weekly_limit) barClass += " over-pace";

    barsHtml += `
      <g class="${barClass}">
        <rect x="${barXPercent}%" y="${100 - barHeightPercent}%" width="${barWidthPercent}%" height="${barHeightPercent}%" rx="2"/>
      </g>
    `;

    // Day label (HTML, not SVG)
    const labelClass = day.is_today ? "day-label today" : "day-label";
    labelsHtml += `<span class="${labelClass}">${day.day_name}</span>`;
  }

  return `
    <div class="weekly-usage-chart">
      <svg viewBox="0 0 100 100" preserveAspectRatio="none">
        <!-- Pace line (target trajectory) -->
        <polyline class="pace-line" points="${paceLinePoints}" fill="none"/>
        <!-- Bars -->
        ${barsHtml}
      </svg>
      <div class="weekly-day-labels">${labelsHtml}</div>
      <div class="weekly-legend">
        <span class="legend-item"><span class="legend-bar"></span>Cumulative</span>
        <span class="legend-item"><span class="legend-line"></span>Target pace</span>
      </div>
    </div>
  `;
}

function toggleSettings(): void {
  settingsOpen = !settingsOpen;
  const panel = document.getElementById("settings-panel");
  if (panel) {
    panel.style.display = settingsOpen ? "block" : "none";
  }
}

function scheduleRetry(): void {
  if (retryCount >= MAX_RETRIES) return;

  const delay = BASE_RETRY_DELAY_MS * Math.pow(2, retryCount);
  retryCount++;

  if (retryTimeoutId) clearTimeout(retryTimeoutId);
  retryTimeoutId = setTimeout(() => {
    retryTimeoutId = null;
    fetchUsage();
  }, delay);
}

async function fetchUsage(): Promise<void> {
  // Skip updates during ongoing render to maintain responsiveness
  if (isRendering) return;

  const statsEl = document.getElementById("stats");
  const errorEl = document.getElementById("error");
  const loadingEl = document.getElementById("loading");

  if (!statsEl || !errorEl || !loadingEl) return;

  try {
    isRendering = true;
    // Safety timeout to reset isRendering if it gets stuck
    const renderTimeout = setTimeout(() => {
      isRendering = false;
    }, 5000);

    const stats: UsageStats = await invoke("get_usage", { period: "today" });
    clearTimeout(renderTimeout);

    // Success - reset retry state and clear reload flag
    retryCount = 0;
    if (retryTimeoutId) {
      clearTimeout(retryTimeoutId);
      retryTimeoutId = null;
    }
    sessionStorage.removeItem("cc-widget-reload-attempted");

    // Use requestAnimationFrame to batch DOM updates at the next paint cycle
    requestAnimationFrame(() => {
      loadingEl.style.display = "none";
      errorEl.style.display = "none";

      const quotaColor = getQuotaColor(stats.quota.usage_percent);
      const weekColor = getQuotaColor(stats.quota.week_usage_percent);

      statsEl.innerHTML = `
      <div class="quota-section">
        <div class="quota-row">
          <div class="quota-item">
            <div class="quota-header">
              <span class="quota-title">Rolling ${stats.quota.window_hours}hr Limit</span>
            </div>
            <div class="quota-bar-container">
              <div class="quota-bar" style="width: ${stats.quota.usage_percent}%; background: ${quotaColor};"></div>
            </div>
            <div class="quota-details">
              <span class="quota-percent" style="color: ${quotaColor};">${stats.quota.usage_percent.toFixed(1)}%</span>
              <span class="quota-count">${stats.quota.messages_in_window}/${stats.quota.estimated_limit}</span>
            </div>
          </div>
          <div class="quota-item">
            <div class="quota-header">
              <span class="quota-title">Weekly Limit</span>
              <span class="quota-reset">Reset ${getNextWeeklyReset()}</span>
            </div>
            <div class="quota-bar-container">
              <div class="quota-bar" style="width: ${stats.quota.week_usage_percent}%; background: ${weekColor};"></div>
            </div>
            <div class="quota-details">
              <span class="quota-percent" style="color: ${weekColor};">${stats.quota.week_usage_percent.toFixed(1)}%</span>
              <span class="quota-count">${stats.quota.plan}</span>
            </div>
          </div>
        </div>
      </div>

      <div class="weekly-section">
        <h3>Weekly Usage</h3>
        ${renderWeeklyUsageChart(stats.weekly_usage)}
      </div>

      <div class="activity-section">
        <h3>Activity (12 weeks)</h3>
        ${renderActivityHeatmap(stats.daily_activity)}
      </div>

      <div class="model-breakdown">
        <h3>Models</h3>
        ${stats.by_model.length > 0 ? stats.by_model
          .map(
            (m) => {
              const totalTokens = m.tokens.input_tokens + m.tokens.output_tokens +
                m.tokens.cache_read_input_tokens + m.tokens.cache_creation_input_tokens;
              return `
          <div class="model-row ${getModelClass(m.model)}">
            <div class="model-info">
              <span class="model-name">${m.display_name}</span>
            </div>
            <span class="model-tokens">${formatNumber(totalTokens)} tokens</span>
          </div>
        `;
            }
          )
          .join("") : '<div class="model-row"><span class="muted">No data</span></div>'}
      </div>

      <div class="sessions-section">
        <h3>Active Sessions (24hr)</h3>
        <div class="sessions-list">
          ${stats.active_sessions.length > 0 ? stats.active_sessions
            .map(
              (s) => `
            <div class="session-row">
              <span class="session-directory" title="${s.directory}">${formatDirectory(s.directory)}</span>
              <span class="session-model ${getModelClass(s.model)}">${s.model_display_name}</span>
              <span class="session-context">${s.context_remaining_percent.toFixed(0)}%</span>
              <span class="session-todos">${s.todo_count > 0 ? s.todo_count : "-"}</span>
              <span class="session-duration">${formatDuration(s.duration_minutes)}</span>
            </div>
          `
            )
            .join("") : '<div class="session-row"><span class="muted">No active sessions</span></div>'}
        </div>
      </div>

      <div class="last-updated">
        ${stats.last_updated ? new Date(stats.last_updated).toLocaleTimeString() : "—"}
      </div>
    `;

      applyTransparency();
      isRendering = false;
    });
  } catch (e) {
    isRendering = false;
    loadingEl.style.display = "none";
    errorEl.style.display = "block";

    // Log detailed error information for debugging
    console.error("fetchUsage error:", e);
    console.error("Error type:", typeof e);
    if (e instanceof Error) {
      console.error("Error name:", e.name, "message:", e.message, "stack:", e.stack);
    }

    const errorStr = String(e);
    const isConnectionError = errorStr.includes("localhost") || errorStr.includes("Connection");

    if (retryCount < MAX_RETRIES) {
      const nextDelay = (BASE_RETRY_DELAY_MS * Math.pow(2, retryCount)) / 1000;
      errorEl.textContent = `Connection error. Retrying in ${nextDelay.toFixed(0)}s...`;
      scheduleRetry();
    } else if (isConnectionError && !reloadAttempted) {
      // WebKit IPC may be broken - try a page reload to recover
      console.log("Connection errors exhausted retries, attempting page reload to recover WebKit");
      sessionStorage.setItem("cc-widget-reload-attempted", "true");
      errorEl.textContent = "Reloading to recover...";
      setTimeout(() => window.location.reload(), 500);
    } else {
      errorEl.textContent = `${e}`;
    }
  }
}

async function setupFileWatcher(): Promise<void> {
  try {
    await listen("usage-updated", () => {
      fetchUsage();
    });
  } catch (e) {
    console.error("Failed to set up file watcher:", e);
  }
}

async function setupSuspendHandler(): Promise<void> {
  try {
    await listen("system-resumed", () => {
      // System just resumed from suspend - WebKit processes may be stale.
      // Reset retry state and attempt to recover by reloading the page.
      console.log("System resumed from suspend, reloading to recover WebKit state");
      retryCount = 0;
      if (retryTimeoutId) {
        clearTimeout(retryTimeoutId);
        retryTimeoutId = null;
      }
      // Short delay to let system stabilize after resume
      setTimeout(() => {
        window.location.reload();
      }, 500);
    });
  } catch (e) {
    console.error("Failed to set up suspend handler:", e);
  }
}

async function setupTitleBar(): Promise<void> {
  const closeBtn = document.getElementById("close-btn");
  const minimizeBtn = document.getElementById("minimize-btn");
  const titleBar = document.getElementById("title-bar");
  const container = document.querySelector(".container");
  const appWindow = getCurrentWindow();

  closeBtn?.addEventListener("click", async () => {
    await appWindow.close();
  });

  minimizeBtn?.addEventListener("click", async () => {
    await appWindow.minimize();
  });


  // Enable dragging on title bar (fallback for Linux)
  if (titleBar) {
    titleBar.addEventListener("mousedown", (e) => {
      // Only drag if clicking on the title bar itself, not buttons
      if ((e.target as HTMLElement).closest(".title-buttons")) return;
      if ((e.target as HTMLElement).closest(".settings-panel")) return;
      if (e.button === 0) { // Left mouse button only
        e.preventDefault();
        // Fire-and-forget to avoid blocking
        appWindow.startDragging();
      }
    });
  }
}

function setupSettings(): void {
  const settingsBtn = document.getElementById("settings-btn");
  const transparencySlider = document.getElementById("transparency-slider") as HTMLInputElement;
  const transparencyValue = document.getElementById("transparency-value");

  settingsBtn?.addEventListener("click", toggleSettings);

  if (transparencySlider) {
    transparencySlider.value = String(transparency);
    transparencySlider.addEventListener("input", () => {
      transparency = parseInt(transparencySlider.value);
      if (transparencyValue) {
        transparencyValue.textContent = `${transparency}%`;
      }
      applyTransparency();
      saveSettings();
    });
  }

  if (transparencyValue) {
    transparencyValue.textContent = `${transparency}%`;
  }
}

async function logWebKitEnv(): Promise<void> {
  try {
    const env = await invoke<Record<string, string>>("get_webkit_env");
    console.log("WebKit environment variables:", env);
    const expected = [
      "WEBKIT_DISABLE_COMPOSITING_MODE",
      "WEBKIT_DISABLE_DMABUF_RENDERER",
      "WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS",
      "WEBKIT_USE_SINGLE_WEB_PROCESS",
      "WEBKIT_DISABLE_GPU",
    ];
    const missing = expected.filter((v) => !(v in env));
    if (missing.length > 0) {
      console.warn("Missing WebKit environment variables:", missing);
      // Show warning in UI for debugging
      const errorEl = document.getElementById("error");
      if (errorEl) {
        errorEl.style.display = "block";
        errorEl.textContent = `Missing WebKit env: ${missing.join(", ")}`;
      }
    }
  } catch (e) {
    console.error("Failed to get WebKit env:", e);
  }
}

window.addEventListener("DOMContentLoaded", () => {
  loadSettings();
  setupTitleBar();
  setupSettings();

  document.getElementById("refresh-btn")?.addEventListener("click", fetchUsage);

  // Delay before first invoke to ensure WebKit IPC is fully initialized
  setTimeout(() => {
    fetchUsage();
    setupFileWatcher();
    setupSuspendHandler();
    // Refresh data every 30 seconds (reduced from 10s to minimize IPC load)
    setInterval(fetchUsage, 30000);
  }, 500);
});
