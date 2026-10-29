import { selectDisplayQuotaWindows, getRemainingPercent, statusTone, type CoreState } from './core-state';

export type AlertLevel = 'warning' | 'critical' | 'depleted';

export function quotaAlertState(state: CoreState | null): AlertLevel | 'healthy' | null {
  if (!state || state.connection !== 'ready' || state.stale || state.error ||
      state.ordinaryUsageAllowed === false) return null;
  const window = selectDisplayQuotaWindows(state.windows, state.account).primary;
  if (!window || window.remainingPercent === null || !Number.isFinite(window.remainingPercent) ||
      window.remainingPercent < 0 || window.remainingPercent > 100) return null;
  const remaining = getRemainingPercent(window);
  if (remaining === 0) return 'depleted';
  const tone = statusTone(state, remaining);
  return tone === 'neutral' ? null : tone;
}

export function createQuotaAlerts() {
  let previous: ReturnType<typeof quotaAlertState> = null;
  return (state: CoreState | null, enabled: boolean): AlertLevel | null => {
    const next = quotaAlertState(state);
    // Invalid snapshots cannot prove a quota transition; preserve dedup state.
    if (next === null) return null;
    const changed = next !== previous;
    previous = next;
    return enabled && changed && next !== 'healthy' ? next : null;
  };
}
