import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import ts from 'typescript';
function load(path, imports = {}) {
  const source = readFileSync(new URL(path, import.meta.url), 'utf8');
  const { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS } });
  const module = { exports: {} };
  new Function('require', 'module', 'exports', outputText)(id => imports[id], module, module.exports);
  return module.exports;
}
const core = load('../src/core-state.ts');
const { createQuotaAlerts, quotaAlertState } = load('../src/quota-alerts.ts', { './core-state': core });
const fixtures = load('../src/gate4-notification-fixtures.ts', { './core-state': core });
const state = (remaining = 20) => ({ connection: 'ready', error: null, stale: false,
  ordinaryUsageAllowed: true, account: { planType: 'pro' },
  windows: [{ limitId: 'codex', kind: 'weekly', remainingPercent: remaining }] });
test('alerts reuse health classification and distinguish depletion', () => {
  for (const n of [100, 26, 25, 11, 10, 1]) assert.equal(quotaAlertState(state(n)), core.statusTone(state(n), n));
  assert.equal(quotaAlertState(state(0)), 'depleted');
});
test('consecutive states deduplicate; transitions including healthy rearm', () => {
  const next = createQuotaAlerts();
  assert.equal(next(state(), true), 'warning');
  assert.equal(next(state(), true), null);
  assert.equal(next(state(10), true), 'critical');
  assert.equal(next(state(0), true), 'depleted');
  assert.equal(next(state(80), true), null);
  assert.equal(next(state(), true), 'warning');
});
test('all errors, stale, offline, restricted and invalid quota are silent', () => {
  const invalid = [null, { ...state(), stale: true }, { ...state(), connection: 'offline' },
    { ...state(), ordinaryUsageAllowed: false }, { ...state(), windows: [] },
    ...['OFFLINE','NOT_AUTHENTICATED','CODEX_NOT_FOUND','UNSUPPORTED_CODEX'].map(error => ({ ...state(), error })),
    ...[null, NaN, Infinity, -1, 101].map(state)];
  for (const value of invalid) assert.equal(createQuotaAlerts()(value, true), null);
});
test('invalid snapshots do not rearm unchanged quota, disabled alerts stay silent', () => {
  const next = createQuotaAlerts();
  assert.equal(next(state(), true), 'warning');
  assert.equal(next({ ...state(), stale: true }, true), null);
  assert.equal(next(state(), true), null);
  assert.equal(next(state(10), false), null);
  assert.equal(next(state(10), true), null);
  assert.equal(next(state(0), true), 'depleted');
});
test('main quota alerts ignore model-specific buckets', () => {
  const codexWeekly = { limitId: 'codex', kind: 'weekly', usedPercent: 80, remainingPercent: 20 };
  const modelFive = { limitId: 'codex_bengalfox', kind: 'five_hour', usedPercent: 0, remainingPercent: 100 };
  const modelWeekly = { limitId: 'codex_bengalfox', kind: 'weekly', usedPercent: 0, remainingPercent: 100 };
  assert.equal(quotaAlertState({ ...state(), windows: [modelFive, modelWeekly, codexWeekly] }), 'warning');
  assert.equal(quotaAlertState({ ...state(), windows: [modelFive, modelWeekly] }), null);
});
test('Gate 4 native fixtures drive the existing classifier without a second threshold table', () => {
  for (const [mode, expected] of [
    ['healthy', 'healthy'], ['warning', 'warning'], ['critical', 'critical'], ['depleted', 'depleted'],
    ['stale', null], ['error', null], ['offline', null], ['restricted', null], ['no_quota', null],
  ]) assert.equal(quotaAlertState(fixtures.createGate4NotificationState(mode, 1_800_000_000)), expected);
  const next = createQuotaAlerts();
  const warning = () => fixtures.createGate4NotificationState('warning', 1_800_000_000);
  assert.equal(next(warning(), true), 'warning');
  assert.equal(next(warning(), true), null);
});
test('old notification preview is development gated and native warning entry is permanently retired', () => {
  const main = readFileSync(new URL('../src/main.tsx', import.meta.url), 'utf8');
  const cargo = readFileSync(new URL('../src-tauri/Cargo.toml', import.meta.url), 'utf8');
  const native = readFileSync(new URL('../src-tauri/src/desktop.rs', import.meta.url), 'utf8');
  assert.match(main, /import\.meta\.env\.DEV && import\.meta\.env\.VITE_GATE4_ACCEPTANCE === '1'/);
  assert.match(cargo, /gate4-acceptance = \[\]/);
  assert.match(native, /show_quota_notification_acceptance\(\)[^{]*\{\s*Err\("QUOTA_ALERT_WINDOW_RETIRED"/);
});
test('normal runtime has no independent quota warning window trigger', () => {
  const main = readFileSync(new URL('../src/main.tsx', import.meta.url), 'utf8');
  const desktop = readFileSync(new URL('../src/desktop.tsx', import.meta.url), 'utf8');
  const lib = readFileSync(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf8');
  const native = readFileSync(new URL('../src-tauri/src/desktop.rs', import.meta.url), 'utf8');
  assert.doesNotMatch(main, /useQuotaNotifications/);
  assert.doesNotMatch(desktop, /export function useQuotaNotifications\b/);
  assert.doesNotMatch(lib, /desktop::show_quota_notification,/);
  assert.doesNotMatch(native, /pub async fn show_quota_notification\(/);
  assert.match(native, /show_quota_notification_acceptance/);
  assert.match(native, /QUOTA_ALERT_WINDOW_RETIRED/);
  assert.doesNotMatch(native, /fn show_quota_notification_window/);
  const windows = readFileSync(new URL('../src-tauri/src/utility_windows.rs', import.meta.url), 'utf8');
  assert.doesNotMatch(windows, /WebviewWindowBuilder::new\([^\n]*"quota-alert"/);
});
