export type UtilityIconName = 'general' | 'appearance' | 'bell' | 'keyboard' | 'info' | 'window' | 'eye' | 'eye-off' | 'magnet' | 'pin' | 'gear' | 'power' | 'close' | 'chevron' | 'folder' | 'reset';
const paths: Record<UtilityIconName, string> = {
  general: 'M4 7h16M4 17h16M9 4v6M15 14v6',
  appearance: 'M12 3a9 9 0 1 0 9 9c0-1.2-1-2-2.1-2H17a2 2 0 0 1-2-2V6c0-1.8-1.2-3-3-3ZM7 10h.01M8 16h.01M12 7h.01',
  bell: 'M5 17h14l-2-3V9a5 5 0 0 0-10 0v5ZM10 20h4',
  keyboard: 'M4 5h16v14H4ZM7 9h.01M12 9h.01M17 9h.01M7 12h.01M12 12h.01M17 12h.01M8 16h8',
  info: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18ZM12 11v6M12 7h.01',
  window: 'M3 4h18v13H3ZM8 21h8M12 17v4',
  eye: 'M2 12s3-7 10-7 10 7 10 7-3 7-10 7S2 12 2 12Zm10-3a3 3 0 1 0 0 6 3 3 0 0 0 0-6',
  'eye-off': 'M3 3l18 18M9 5.5A12 12 0 0 1 12 5c7 0 10 7 10 7a17 17 0 0 1-3 4M6 6C3.5 8 2 12 2 12s3 7 10 7c1.5 0 3-.3 4-1M9 10a3 3 0 0 0 5 4',
  magnet: 'M5 4v9a7 7 0 0 0 14 0V4h-5v9a2 2 0 0 1-4 0V4ZM5 9h5M14 9h5',
  pin: 'M7 4h10M8 4v5l-4 4v2h16v-2l-4-4V4M12 15v6',
  gear: 'M9.5 4 10 2h4l.5 2 2 1 2-.5 2 3.5-1.5 1.5v3l1.5 1.5-2 3.5-2-.5-2 1-.5 2h-4l-.5-2-2-1-2 .5-2-3.5L5 12.5v-3L3.5 8l2-3.5 2 .5ZM12 8a3.5 3.5 0 1 0 0 7 3.5 3.5 0 0 0 0-7',
  power: 'M12 3v8M7 5a8 8 0 1 0 10 0',
  close: 'm6 6 12 12M18 6 6 18',
  chevron: 'm8 10 4 4 4-4',
  folder: 'M3 6h7l2 2h9v12H3ZM3 6V4h6l2 2',
  reset: 'M4 4v6h6M4 10a8 8 0 1 1 1 8',
};
export function UtilityIcon({ name }: { name: UtilityIconName }) {
  return <svg className="utility-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.55" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>;
}
