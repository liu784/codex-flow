export type CoreErrorCode =
  | "CODEX_NOT_FOUND"
  | "NOT_AUTHENTICATED"
  | "UNSUPPORTED_CODEX"
  | "OFFLINE";

export type ConnectionState = "disconnected" | "connecting" | "ready" | "offline";
export type QuotaWindowKind = "five_hour" | "weekly" | "custom" | "unknown";

export interface AccountState {
  authenticated: boolean;
  requiresOpenaiAuth: boolean;
  accountType: string | null;
  planType: string | null;
}

export interface QuotaWindow {
  limitId: string | null;
  planType: string | null;
  sourceSlot: string;
  kind: QuotaWindowKind;
  durationMinutes: number | null;
  usedPercent: number | null;
  remainingPercent: number | null;
  resetsAt: number | null;
}

export interface CoreState {
  connection: ConnectionState;
  error: CoreErrorCode | null;
  account: AccountState | null;
  ordinaryUsageAllowed: boolean | null;
  resetCreditsAvailable: number | null;
  windows: QuotaWindow[];
  stale: boolean;
  lastSuccessAt: number | null;
}

export interface SelectedQuotaWindows {
  primary: QuotaWindow | null;
  primaryKind: QuotaWindowKind | null;
  secondaryWeekly: QuotaWindow | null;
}

export interface ErrorMessage {
  title: string;
  detail: string;
}

const ERROR_MESSAGES: Record<CoreErrorCode, ErrorMessage> = {
  CODEX_NOT_FOUND: {
    title: "未找到 Codex",
    detail: "请先安装 Codex，再重新连接。",
  },
  NOT_AUTHENTICATED: {
    title: "尚未登录 Codex",
    detail: "请在 Codex 中完成登录后刷新。",
  },
  UNSUPPORTED_CODEX: {
    title: "Codex 版本不受支持",
    detail: "请更新 Codex 后重新连接。",
  },
  OFFLINE: {
    title: "Codex 服务暂时离线",
    detail: "保留最近数据，可稍后重试。",
  },
};

export function selectQuotaWindows(
  windows: readonly QuotaWindow[],
): SelectedQuotaWindows {
  // Rust derives kind from duration. Plan metadata must never hide a real window.
  const fiveHour = windows.find((window) => window.kind === "five_hour") ?? null;
  const weeklyWindows = windows.filter((window) => window.kind === "weekly");
  const matchingWeekly = fiveHour?.limitId
    ? weeklyWindows.find((window) => window.limitId === fiveHour.limitId)
    : undefined;
  const weekly = matchingWeekly ?? weeklyWindows[0] ?? null;

  if (fiveHour) {
    return {
      primary: fiveHour,
      primaryKind: "five_hour",
      secondaryWeekly: weekly,
    };
  }

  const primary = weekly ?? windows.find(
    (window) => window.kind === "custom" || window.kind === "unknown",
  ) ?? null;
  return {
    primary,
    primaryKind: primary?.kind ?? null,
    secondaryWeekly: null,
  };
}

export function selectMainQuotaWindows(
  windows: readonly QuotaWindow[],
): SelectedQuotaWindows {
  const mainCodexWindows = windows.filter(
    (window) => window.limitId?.trim().toLowerCase() === "codex",
  );
  return selectQuotaWindows(mainCodexWindows);
}

export function isProPlan(account: AccountState | null): boolean {
  const plan = account?.planType?.trim().toLowerCase();
  return plan === "pro" || plan === "prolite";
}

export function selectDisplayQuotaWindows(
  windows: readonly QuotaWindow[],
  account: AccountState | null,
): SelectedQuotaWindows {
  const selected = selectMainQuotaWindows(windows);
  if (!isProPlan(account)) {
    return selected;
  }

  const weekly = selected.primaryKind === "weekly"
    ? selected.primary
    : selected.secondaryWeekly;

  return {
    primary: weekly,
    primaryKind: "weekly",
    secondaryWeekly: null,
  };
}

export function getRemainingPercent(window: QuotaWindow | null): number | null {
  if (
    window?.remainingPercent === null
    || window?.remainingPercent === undefined
    || !Number.isFinite(window.remainingPercent)
  ) {
    return null;
  }
  return Math.round(Math.min(100, Math.max(0, window.remainingPercent)));
}

export function formatPlan(account: AccountState | null): string {
  const rawPlan = account?.planType?.trim();
  if (!rawPlan) {
    return "CODEX";
  }

  const plan = rawPlan.toLowerCase();
  const knownPlans: Record<string, string> = {
    free: "FREE",
    plus: "PLUS",
    pro: "PRO",
    prolite: "PRO",
    team: "TEAM",
    business: "BUSINESS",
    enterprise: "ENTERPRISE",
  };

  return `CODEX · ${knownPlans[plan] ?? rawPlan.toUpperCase()}`;
}

export function getErrorMessage(error: CoreErrorCode | null): ErrorMessage | null {
  return error ? ERROR_MESSAGES[error] : null;
}

export function formatResetTime(timestampSeconds: number | null, now = Date.now()): string {
  if (timestampSeconds === null || !Number.isFinite(timestampSeconds)) {
    return "重置时间不可用";
  }

  const resetAt = timestampSeconds * 1_000;
  const remainingMinutes = Math.max(0, Math.ceil((resetAt - now) / 60_000));

  if (remainingMinutes === 0) {
    return "即将重置";
  }
  if (remainingMinutes < 60) {
    return `${remainingMinutes} 分钟后重置`;
  }
  if (remainingMinutes < 48 * 60) {
    const hours = Math.floor(remainingMinutes / 60);
    const minutes = remainingMinutes % 60;
    return minutes === 0 ? `${hours} 小时后重置` : `${hours} 小时 ${minutes} 分后重置`;
  }

  const date = new Intl.DateTimeFormat("zh-CN", {
    month: "numeric",
    day: "numeric",
  }).format(new Date(resetAt));
  return `${date} 重置`;
}

export function formatRemainingHours(remainingPercent: number | null): string {
  if (remainingPercent === null) {
    return "5 小时额度不可用";
  }
  const hours = ((remainingPercent / 100) * 5).toFixed(1);
  return `剩余 ${hours} 小时 / 5 小时`;
}

export function activityLabel(state: CoreState | null): string {
  if (!state || state.connection === "connecting") {
    return "正在连接";
  }
  if (state.stale) {
    return "数据可能已过期";
  }
  if (state.ordinaryUsageAllowed === false) {
    return "当前使用受限";
  }
  const hasRecentUsage = state.windows.some(
    (window) => window.usedPercent !== null && window.usedPercent > 0,
  );
  return hasRecentUsage ? "近期检测到消耗" : "暂未检测到消耗";
}

export function statusTone(
  state: CoreState | null,
  remainingPercent: number | null,
): "healthy" | "warning" | "critical" | "neutral" {
  if (!state || state.connection === "connecting") {
    return "neutral";
  }
  if (state.ordinaryUsageAllowed === false) {
    return "critical";
  }
  if (state.error || state.stale) {
    return "warning";
  }
  if (remainingPercent === null) {
    return "neutral";
  }
  if (remainingPercent <= 10) {
    return "critical";
  }
  if (remainingPercent <= 25) {
    return "warning";
  }
  return "healthy";
}
