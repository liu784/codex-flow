import { useEffect, useSyncExternalStore } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface AppSettings {
  settingsSchemaVersion: number; autoStart: boolean; alwaysOnTop: boolean;
  minimizeToTray: boolean; mainPanelEnabled?: boolean; autoSnapTopCenter?: boolean; orbHoverReveal: boolean; orbEdgeDockEnabled: boolean;
  orbHalfHide: boolean; marquee: boolean; eyeTracking: boolean;
  panelOpeningAnimation: boolean; reduceMotion: boolean; quotaAlertsEnabled: boolean;
  warningEnabled: boolean; criticalEnabled: boolean; refreshInterval: number;
}
export type SettingsPatch = Partial<Omit<AppSettings, 'settingsSchemaVersion'>>;
type Snapshot = { settings: AppSettings | null; error: string; saving: boolean };
let snapshot: Snapshot = { settings: null, error: '', saving: false };
const subscribers = new Set<() => void>();
let started: Promise<void> | undefined;
let revision = 0;
let pending = 0;
function publish(next: Partial<Snapshot>) { snapshot = { ...snapshot, ...next }; subscribers.forEach(fn => fn()); }
export async function reloadSettings() {
  const before = revision;
  try {
    const settings = await invoke<AppSettings>('get_settings');
    if (before === revision) publish({ settings, error: '' });
  } catch { publish({ error: '无法读取设置，请重新打开设置窗口' }); }
}
async function start() {
  if (!isTauri()) { publish({ error: '请在 Codex Flow 中打开设置' }); return; }
  await listen<AppSettings>('settings-changed', event => {
    revision++; publish({ settings: event.payload, error: '' });
  });
  await reloadSettings();
  window.addEventListener('focus', () => { void reloadSettings(); });
}
export function useAppSettings() {
  useEffect(() => { started ??= start().catch(() => publish({ error: '无法连接设置服务' })); }, []);
  return useSyncExternalStore(fn => { subscribers.add(fn); return () => { subscribers.delete(fn); }; }, () => snapshot);
}
export function currentSettings() { return snapshot.settings; }
async function update(command: string, args: Record<string, unknown>) {
  pending++; publish({ saving: true });
  try {
    // Backend events own ordering; do not overwrite a newer event with an older command response.
    const before = revision;
    const settings = await invoke<AppSettings>(command, args);
    if (revision === before) publish({ settings, error: '' });
    return settings;
  } catch (error) { publish({ error: '设置未能应用或保存，请重试' }); throw error; }
  finally { pending--; publish({ saving: pending > 0 }); }
}
export function changeSettings(patch: SettingsPatch) { return update('set_settings', { patch }); }
export function resetSettings() { return update('reset_settings', { confirmed: true }); }
