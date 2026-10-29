import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { changeSettings, useAppSettings } from './app-settings';
import { UtilityIcon, type UtilityIconName } from './components/UtilityIcon';
import { Toggle, useUtilityOpening } from './SettingsPage';

export function TrayPopup() {
  const { settings, saving, error } = useAppSettings();
  const [state, setState] = useState<{ orbVisible: boolean; panelExpanded: boolean } | null>(null);
  const [actionError, setActionError] = useState('');
  const [busy, setBusy] = useState(false);
  const root = useRef<HTMLElement>(null);
  const generation = useUtilityOpening();
  useEffect(() => {
    let gone = false; let remove: (() => void) | undefined;
    const read = () => { void invoke<{ orbVisible: boolean; panelExpanded: boolean }>('get_tray_state').then(s => { if (!gone) setState(s); }).catch(() => { if (!gone) setActionError('无法读取窗口状态'); }); };
    read();
    void listen('tray-state-changed', read).then(fn => { if (gone) fn(); else remove = fn; }).catch(() => {});
    root.current?.focus();
    return () => { gone = true; remove?.(); };
  }, [generation]);
  async function action(name: string) { setBusy(true); try { await invoke('tray_action', { action: name }); setActionError(''); } catch { setActionError('操作未完成，请重试'); } finally { setBusy(false); } }
  function item(icon: UtilityIconName, label: string, name: string) {
    return <button className="tray-item" disabled={busy || (name === 'toggle_orb' && !state)} onClick={() => void action(name)}><UtilityIcon name={icon} /><span>{label}</span></button>;
  }
  return <section ref={root} tabIndex={-1} className={`utility-window tray-window ${settings?.reduceMotion ? 'utility-reduced' : ''}`} aria-label="Codex Flow 托盘菜单"
    onKeyDown={event => {
      if (event.key === 'Escape') { event.preventDefault(); void invoke('close_utility'); }
      if (['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) {
        event.preventDefault(); const buttons = [...(root.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])];
        if (!buttons.length) return;
        const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
        const next = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1
          : at < 0 ? (event.key === 'ArrowDown' ? 0 : buttons.length - 1)
          : (at + (event.key === 'ArrowDown' ? 1 : buttons.length - 1)) % buttons.length;
        buttons[next]?.focus();
      }
    }}>
    <div className="utility-opening tray-opening" key={generation}>
      {item('window', '显示主面板', 'show_panel')}
      {item(state?.orbVisible ? 'eye-off' : 'eye', state?.orbVisible ? '隐藏灵动岛' : '显示灵动岛', 'toggle_orb')}
      <div className="tray-item tray-toggle-row"><UtilityIcon name="pin" /><span>自动吸附</span><Toggle label="自动吸附" checked={settings?.autoSnapTopCenter ?? true} disabled={!settings || saving}
        onChange={() => { void changeSettings({ autoSnapTopCenter: !(settings?.autoSnapTopCenter ?? true) }).catch(() => {}); }} /></div>
      <div className="tray-item tray-toggle-row"><UtilityIcon name="window" /><span>主面板开关</span><Toggle label="主面板开关" checked={settings?.mainPanelEnabled ?? true} disabled={!settings || saving}
        onChange={() => { void changeSettings({ mainPanelEnabled: !(settings?.mainPanelEnabled ?? true) }).catch(() => {}); }} /></div>
      <div className="tray-item tray-toggle-row"><UtilityIcon name="pin" /><span>始终置顶</span><Toggle label="始终置顶" checked={settings?.alwaysOnTop ?? false} disabled={!settings || saving}
        onChange={() => { void changeSettings({ alwaysOnTop: !settings?.alwaysOnTop }).catch(() => {}); }} /></div>
      <hr />{item('gear', '设置', 'settings')}{item('power', '退出', 'exit')}
      {(error || actionError) && <p className="tray-error" role="status">{error || actionError}</p>}
    </div>
  </section>;
}
