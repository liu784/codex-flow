import { useEffect, useRef, useState, type ReactNode } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { changeSettings, resetSettings, useAppSettings, type SettingsPatch } from './app-settings';
import { chooseIslandAvatar, DEFAULT_ISLAND_AVATAR, resetIslandAvatar, useIslandAvatar } from './island-avatar';
import { UtilityIcon, type UtilityIconName } from './components/UtilityIcon';
import './utility-windows.css';

export const SETTINGS_SECTIONS = [
  { id: 'general', label: '常规', icon: 'general', description: '让 Codex Flow 按你的习惯运行。' },
  { id: 'appearance', label: '外观', icon: 'appearance', description: '轻微动态，恰到好处。' },
  { id: 'alerts', label: '提醒', icon: 'bell', description: '通过状态颜色了解剩余额度。' },
  { id: 'shortcuts', label: '快捷键', icon: 'keyboard', description: '当前支持的键盘操作。' },
  { id: 'about', label: '关于', icon: 'info', description: '安静地陪伴每一次专注。' },
] as const;
type Section = typeof SETTINGS_SECTIONS[number]['id'];
export function Toggle({ checked, disabled, label, onChange }: { checked: boolean; disabled?: boolean; label: string; onChange: () => void }) {
  return <button type="button" role="switch" aria-checked={checked} aria-label={label} disabled={disabled}
    className="utility-toggle" onClick={onChange}><span /></button>;
}
export function useUtilityOpening() {
  const [generation, setGeneration] = useState(0);
  useEffect(() => {
    let gone = false; let remove: (() => void) | undefined;
    void listen('utility-open', () => { if (!gone) setGeneration(n => n + 1); })
      .then(fn => { if (gone) fn(); else remove = fn; }).catch(() => {});
    return () => { gone = true; remove?.(); };
  }, []);
  return generation;
}
function Choice({ value, disabled, onChange }: { value: number; disabled: boolean; onChange: (value: number) => void }) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const options = [[0, '自动（推荐）'], [60, '1 分钟'], [300, '5 分钟'], [600, '10 分钟'], [1800, '30 分钟']] as const;
  return <div className="utility-choice" ref={root} onBlur={e => { if (!e.currentTarget.contains(e.relatedTarget as Node)) setOpen(false); }}
    onKeyDown={e => {
      if (e.key === 'Escape' && open) { e.stopPropagation(); setOpen(false); root.current?.querySelector('button')?.focus(); }
      if (['ArrowDown', 'ArrowUp'].includes(e.key)) {
        e.preventDefault(); setOpen(true);
        const buttons = [...(root.current?.querySelectorAll<HTMLButtonElement>('[role="option"]') ?? [])];
        const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
        buttons[(index + (e.key === 'ArrowDown' ? 1 : buttons.length - 1)) % buttons.length]?.focus();
      }
    }}>
    <button className="utility-choice-trigger" disabled={disabled} aria-label="更新间隔" aria-haspopup="listbox" aria-expanded={open}
      onClick={() => setOpen(v => !v)}>{options.find(o => o[0] === value)?.[1] ?? '自动（推荐）'}<UtilityIcon name="chevron" /></button>
    {open && <div className="utility-choice-list" role="listbox" aria-label="更新间隔">{options.map(([seconds, label]) =>
      <button key={seconds} role="option" aria-selected={value === seconds} onClick={() => { onChange(seconds); setOpen(false); root.current?.querySelector('button')?.focus(); }}>{label}</button>)}</div>}
  </div>;
}
type NotificationStatus = { enabled: boolean };
function NotificationControls() {
  const [status, setStatus] = useState<NotificationStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [connection, setConnection] = useState('');
  const refresh = () => invoke<NotificationStatus>('get_island_notification_status').then(setStatus).catch(() => setError('无法读取通知状态'));
  useEffect(() => {
    const onFocus = () => { void refresh(); };
    onFocus(); window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, []);
  const toggle = () => {
    setBusy(true); setError('');
    const action = status?.enabled
      ? invoke('disable_island_notifications').then(() => refresh())
      : invoke<NotificationStatus>('enable_island_notifications').then(setStatus);
    void action.catch(() => setError('无法开启消息提醒，请重试')).finally(() => setBusy(false));
  };
  const hint = status?.enabled
    ? '已开启本机消息提醒；无需开启 Windows 系统通知。'
    : '默认关闭；开启后才会接收本机新消息。';
  const connectCodex = () => {
    setBusy(true); setError(''); setConnection('');
    void invoke<'installed' | 'existing'>('connect_codex_notifications').then(result => {
      setConnection(result === 'installed'
        ? '已添加配置。请在 Codex 的 Hooks 设置中审核并启用，然后重启 Codex。'
        : '已找到 Codex Flow 配置。请在 Codex 的 Hooks 设置中确认已启用。');
    }).catch((reason: unknown) => {
      const code = String(reason);
      setError(code.includes('CONFIG_INVALID')
        ? '现有 Codex 配置无法读取，未作修改；请先检查配置文件。'
        : code.includes('CONFIG_CHANGED') ? 'Codex 配置刚被更改，请重试。' : '无法配置 Codex 提醒，请检查文件权限后重试。');
    }).finally(() => setBusy(false));
  };
  return <div className="settings-notifications">
    <div className="settings-row"><span className="settings-row-copy"><span>应用消息提醒</span><small>Codex、Grok Bot</small></span>
      <button className="settings-notification-button" type="button" disabled={!status || busy} onClick={toggle}>
        {status?.enabled ? '关闭' : '启用'}
      </button>
    </div>
    <p className="settings-notification-hint" role="status">{error || hint}</p>
    <div className="settings-row"><span className="settings-row-copy"><span>连接 Codex</span><small>添加当前用户的回复提醒配置</small></span>
      <button className="settings-notification-button" type="button" disabled={busy} onClick={connectCodex}>配置</button>
    </div>
    {connection && <p className="settings-notification-hint" role="status">{connection}</p>}
    <p className="settings-notification-privacy">Grok Bot 登录后自动读取本机新回复；Codex 需配置并在 Codex 中审核启用。内容只在本机显示，不另行保存。</p>
  </div>;
}
export function SettingsPage() {
  const { settings, saving, error } = useAppSettings();
  const avatarState = useIslandAvatar();
  const [section, setSection] = useState<Section>('general');
  const [info, setInfo] = useState<{ version: string; runtime: string } | null>(null);
  const [localError, setLocalError] = useState('');
  const [confirmReset, setConfirmReset] = useState(false);
  const confirmation = useRef<HTMLDivElement>(null);
  const avatarInput = useRef<HTMLInputElement>(null);
  const generation = useUtilityOpening();
  const active = SETTINGS_SECTIONS.find(s => s.id === section)!;
  const unavailable = !settings || saving;
  useEffect(() => { void invoke<{ version: string; runtime: string }>('get_app_info').then(setInfo).catch(() => setLocalError('无法读取版本信息')); }, []);
  useEffect(() => { if (confirmReset) confirmation.current?.querySelector<HTMLButtonElement>('button')?.focus(); }, [confirmReset]);
  const patch = (next: SettingsPatch) => { void changeSettings(next).catch(() => {}); };
  function row(key: keyof SettingsPatch, title: string, note?: string, disabled = false) {
    return <div className="settings-row" key={key}><span className="settings-row-copy"><span>{title}</span>{note && <small>{note}</small>}</span>
      <Toggle checked={Boolean(settings?.[key])} disabled={unavailable || disabled} label={title} onChange={() => patch({ [key]: !settings?.[key] })} /></div>;
  }
  let content: ReactNode;
  if (section === 'general') content = <>
    {row('autoStart', '开机启动', '登录 Windows 后启动')}
    {row('alwaysOnTop', '始终置顶')}
    {row('minimizeToTray', '关闭后驻留托盘')}
    {row('autoSnapTopCenter', '自动吸附', '拖动后自动恢复到屏幕正上方居中位置')}
    {row('mainPanelEnabled', '主面板开关', '关闭后点击或悬停灵动岛不弹出主面板，防顶部误触')}
    {row('orbHoverReveal', '鼠标悬停展开', '悬停在灵动岛上时展开面板', settings?.mainPanelEnabled === false)}
    <div className="settings-row"><span>更新间隔</span><Choice value={settings?.refreshInterval ?? 0} disabled={unavailable} onChange={refreshInterval => patch({ refreshInterval })} /></div>
    <div className="settings-row settings-language"><span>语言</span><span>简体中文</span></div>
  </>;
  else if (section === 'appearance') content = <>
    <div className="settings-avatar">
      <img src={avatarState.avatar ?? DEFAULT_ISLAND_AVATAR} alt="当前灵动岛头像" className={avatarState.avatar ? 'is-custom' : ''} />
      <div className="settings-avatar-copy"><span>灵动岛头像</span><small>PNG、JPG 或 WebP，不超过 5 MB</small>
        <div className="settings-avatar-actions">
          <input ref={avatarInput} type="file" accept="image/png,image/jpeg,image/webp" aria-label="选择灵动岛头像" hidden
            onChange={event => { const file = event.currentTarget.files?.[0]; event.currentTarget.value = ''; if (file) void chooseIslandAvatar(file).catch(() => {}); }} />
          <button type="button" disabled={!avatarState.ready || avatarState.saving} onClick={() => avatarInput.current?.click()}>更换图片</button>
          <button type="button" disabled={!avatarState.ready || avatarState.saving || !avatarState.avatar} onClick={() => { void resetIslandAvatar().catch(() => {}); }}>恢复默认</button>
        </div>
      </div>
    </div>
    {row('panelOpeningAnimation', '面板开场动画')}
    {row('reduceMotion', '减少动态效果')}
  </>;
  else if (section === 'alerts') content = <>
    <NotificationControls />
    {row('quotaAlertsEnabled', '低额度状态提示')}
    <div className="settings-threshold"><i className="warning-dot" /><span>剩余额度 ≤25%</span></div>
    {row('warningEnabled', '提醒状态', '沿用当前黄色提示', !settings?.quotaAlertsEnabled)}
    <div className="settings-threshold"><i className="critical-dot" /><span>剩余额度 ≤10%</span></div>
    {row('criticalEnabled', '严重状态', '沿用当前红色提示', !settings?.quotaAlertsEnabled)}
    <p className="settings-footnote">只管理额度状态颜色，不弹出独立警告窗口。连接异常与使用受限提示始终保留。</p>
  </>;
  else if (section === 'shortcuts') content = <>
    <div className="settings-empty"><span className="settings-empty-icon"><UtilityIcon name="keyboard" /></span><h3>暂无自定义快捷键</h3><p>全局快捷键暂未开放。</p></div>
    <div className="shortcut-row"><span>收起当前主面板 / 关闭菜单</span><kbd>Esc</kbd></div>
    <div className="shortcut-row"><span>移动焦点</span><kbd>Tab</kbd></div>
    <div className="shortcut-row"><span>切换当前开关</span><kbd>Space</kbd></div>
  </>;
  else content = <>
    <div className="settings-about-brand"><img src="/assets/brand/tray-icon.png" alt="" /><div><h3>Codex Flow</h3><p>版本 {info?.version ?? '读取中'}</p></div></div>
    <p className="settings-runtime">{info?.runtime ?? '正在读取运行环境'}</p>
    <div className="settings-about-actions"><button onClick={() => { void invoke('open_data_directory').catch(() => setLocalError('无法打开数据目录')); }}><UtilityIcon name="folder" />打开数据目录</button>
      <button disabled={unavailable} onClick={() => setConfirmReset(true)}><UtilityIcon name="reset" />恢复默认设置</button></div>
    <p className="settings-footnote">偏好设置保存在本机，实时应用。</p>
  </>;
  return <section className={`utility-window settings-window ${settings?.reduceMotion ? 'utility-reduced' : ''}`} aria-label="Codex Flow 设置"
    onKeyDown={event => {
      if (event.key === 'Escape') { event.stopPropagation(); if (confirmReset) setConfirmReset(false); else void invoke('close_utility'); }
      if (confirmReset && event.key === 'Tab') {
        const buttons = [...(confirmation.current?.querySelectorAll<HTMLButtonElement>('button') ?? [])];
        const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
        event.preventDefault(); buttons[(at + (event.shiftKey ? buttons.length - 1 : 1)) % buttons.length]?.focus();
      }
    }}>
    <div className="utility-opening settings-opening" key={generation}>
    <header className="settings-header" onPointerDown={event => { if (event.button === 0 && !(event.target as HTMLElement).closest('button')) void invoke('drag_settings').catch(() => {}); }}>
      <span><img src="/assets/brand/tray-icon.png" alt="" />Codex Flow <span className="settings-header-label">设置</span></span>
      <button className="utility-close" aria-label="关闭设置" onClick={() => void invoke('close_utility')}><UtilityIcon name="close" /></button>
    </header>
    <div className="settings-layout"><nav aria-label="设置分类" role="tablist" aria-orientation="vertical">
      {SETTINGS_SECTIONS.map(item => <button key={item.id} id={`nav-${item.id}`} role="tab" aria-selected={section === item.id} aria-controls="settings-content"
        tabIndex={section === item.id ? 0 : -1} onClick={() => setSection(item.id)} onKeyDown={event => {
          if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
          event.preventDefault(); const index = SETTINGS_SECTIONS.findIndex(s => s.id === section);
          const next = event.key === 'Home' ? 0 : event.key === 'End' ? 4 : (index + (event.key === 'ArrowDown' ? 1 : 4)) % 5;
          setSection(SETTINGS_SECTIONS[next].id); document.getElementById(`nav-${SETTINGS_SECTIONS[next].id}`)?.focus();
        }}><UtilityIcon name={item.icon as UtilityIconName} />{item.label}</button>)}
    </nav><main id="settings-content" className="settings-content" role="tabpanel" aria-labelledby={`nav-${section}`} tabIndex={0}>
      <h2>{active.label}</h2><p className="settings-description">{active.description}</p><div className="settings-options">{content}</div>
    </main></div>
    <footer className={`settings-status ${error || localError || avatarState.error ? 'has-error' : ''}`} role="status">{error || localError || avatarState.error || (saving || avatarState.saving ? '正在应用…' : settings ? '已自动保存到本机' : '正在读取设置…')}</footer>
    </div>
    {confirmReset && <div className="settings-confirm-scrim"><div ref={confirmation} className="settings-confirm" role="alertdialog" aria-modal="true" aria-labelledby="reset-title" aria-describedby="reset-description">
      <h3 id="reset-title">恢复默认设置？</h3><p id="reset-description">包括开机启动和动态效果。不会删除额度数据、人物素材或自定义头像。</p>
      {error && <p role="alert">{error}</p>}
      <div><button onClick={() => setConfirmReset(false)}>取消</button><button className="utility-primary" disabled={saving} onClick={() => { void resetSettings().then(() => setConfirmReset(false)).catch(() => {}); }}>恢复默认</button></div>
    </div></div>}
  </section>;
}
