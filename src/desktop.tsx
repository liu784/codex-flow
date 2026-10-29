import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { CoreState } from './core-state';
import { createQuotaAlerts, type AlertLevel } from './quota-alerts';

interface Settings { quotaAlertsEnabled: boolean; orbEdgeDockEnabled: boolean }

function useSettings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState('');
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let revision = 0;
    void (async () => {
      try {
        const remove = await listen<Settings>('settings-changed', event => {
          revision++;
          if (!disposed) setSettings(event.payload);
        });
        if (disposed) { remove(); return; }
        unlisten = remove;
        const before = revision;
        const saved = await invoke<Settings>('get_settings');
        if (!disposed && before === revision) setSettings(saved);
      } catch { if (!disposed) setError('无法读取设置'); }
    })();
    return () => { disposed = true; unlisten?.(); };
  }, []);
  return { settings, setSettings, error, setError };
}

function useQuotaNotificationsWithCommand(state: CoreState | null, command: string) {
  const { settings } = useSettings();
  const next = useRef(createQuotaAlerts());
  const [error, setError] = useState('');
  useEffect(() => {
    if (!settings) return;
    const level = next.current(state, settings.quotaAlertsEnabled);
    if (level) void invoke(command, { level })
      .then(() => setError('')).catch(() => setError('额度提醒发送失败'));
  }, [command, state, settings]);
  return error;
}

export function useGate4AcceptanceNotifications(state: CoreState | null) {
  return useQuotaNotificationsWithCommand(state, 'show_quota_notification_acceptance');
}

export { SettingsPage } from './SettingsPage';

const ALERT_TEXT: Record<AlertLevel, string> = {
  warning: 'Codex 额度偏低', critical: 'Codex 额度即将耗尽', depleted: 'Codex 额度已耗尽',
};
export function NotificationPage() {
  const initial = new URLSearchParams(location.search).get('level') as AlertLevel;
  const [level, setLevel] = useState<AlertLevel>(initial);
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let disposed = false;
    let remove: (() => void) | undefined;
    let seen = 0;
    void listen<AlertLevel>('quota-alert', event => {
      seen++;
      if (!disposed) { setLevel(event.payload); setRevision(n => n + 1); }
    }).then(async unlisten => {
      if (disposed) { unlisten(); return; }
      remove = unlisten;
      const before = seen;
      const latest = await invoke<AlertLevel | null>('get_quota_notification');
      if (!disposed && before === seen && latest) setLevel(latest);
    }).catch(() => { /* Initial URL still supplies the notification text. */ });
    return () => { disposed = true; remove?.(); };
  }, []);
  useEffect(() => {
    const timer = setTimeout(() => { void getCurrentWindow().close(); }, 8000);
    return () => clearTimeout(timer);
  }, [revision]);
  return <section role="status"><h1>{ALERT_TEXT[level] ?? 'Codex 额度提醒'}</h1>
    <button onClick={() => void getCurrentWindow().close()}>关闭</button></section>;
}

export const openSettings = () => invoke('open_settings');
