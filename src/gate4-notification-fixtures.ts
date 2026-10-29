import type { CoreState } from './core-state';

export type Gate4NotificationMode =
  | 'healthy'
  | 'warning'
  | 'critical'
  | 'depleted'
  | 'stale'
  | 'error'
  | 'offline'
  | 'restricted'
  | 'no_quota';

const REMAINING_BY_MODE: Partial<Record<Gate4NotificationMode, number>> = {
  healthy: 80,
  warning: 20,
  critical: 10,
  depleted: 0,
};

export function createGate4NotificationState(
  mode: Gate4NotificationMode,
  now = Math.floor(Date.now() / 1_000),
): CoreState {
  const remaining = REMAINING_BY_MODE[mode] ?? 80;
  const state: CoreState = {
    connection: 'ready',
    error: null,
    account: {
      authenticated: true,
      requiresOpenaiAuth: false,
      accountType: 'chatgpt',
      planType: 'pro',
    },
    ordinaryUsageAllowed: true,
    resetCreditsAvailable: null,
    windows: [{
      limitId: 'codex',
      planType: 'pro',
      sourceSlot: 'primary',
      kind: 'weekly',
      durationMinutes: 10_080,
      usedPercent: 100 - remaining,
      remainingPercent: remaining,
      resetsAt: now + 86_400,
    }],
    stale: false,
    lastSuccessAt: now,
  };

  if (mode === 'stale') state.stale = true;
  if (mode === 'error') state.error = 'CODEX_NOT_FOUND';
  if (mode === 'offline') state.connection = 'offline';
  if (mode === 'restricted') state.ordinaryUsageAllowed = false;
  if (mode === 'no_quota') state.windows = [];
  return state;
}

export function gate4NotificationModeForKey(key: string): Gate4NotificationMode | null {
  return ({
    F1: 'healthy',
    F2: 'warning',
    F3: 'critical',
    F4: 'depleted',
    F5: 'stale',
    F6: 'error',
    F7: 'offline',
    F8: 'restricted',
    F9: 'no_quota',
  } as Record<string, Gate4NotificationMode>)[key] ?? null;
}
