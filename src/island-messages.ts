import { useEffect, useRef, useState } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type IslandMessage = { source: 'Codex' | 'Grok Bot'; text: string };
// Give longer messages enough time to travel through the island and hold at the end.
export function messageDisplayDuration(text: string): number {
  return 6500 + Math.max(0, Array.from(text).length - 8) * 160;
}
function validMessage(value: unknown): IslandMessage | null {
  if (!value || typeof value !== 'object') return null;
  const candidate = value as Record<string, unknown>;
  if (typeof candidate.source !== 'string' || !['Codex', 'Grok Bot'].includes(candidate.source) || typeof candidate.text !== 'string') return null;
  const cleaned = candidate.text.replace(/[\u0000-\u001f\u007f]+/g, ' ').replace(/\s+/g, ' ').trim();
  const text = candidate.source === 'Codex' ? cleaned : Array.from(cleaned).slice(0, 500).join('');
  return text ? { source: candidate.source as IslandMessage['source'], text } : null;
}
export function useIslandMessages(dismiss = false) {
  const [message, setMessage] = useState<IslandMessage | null>(null);
  const clearActive = useRef<(() => void) | null>(null);
  useEffect(() => {
    if (dismiss) clearActive.current?.();
  }, [dismiss]);
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false; const unsubscribers: (() => void)[] = []; let timer: ReturnType<typeof setTimeout> | undefined;
    let current: IslandMessage | null = null;
    const pending: IslandMessage[] = [];
    const recentShown = new Map<string, number>();
    const hideCurrent = () => {
      clearTimeout(timer);
      timer = undefined;
      current = null;
      if (!disposed) setMessage(null);
    };
    const showNext = () => {
      hideCurrent();
      const item = pending.shift();
      if (!item || disposed) return;
      current = item;
      const duration = messageDisplayDuration(item.text);
      recentShown.set(`${item.source}:${item.text}`, Date.now() + Math.max(duration + 8000, 15000));
      setMessage(item);
      timer = setTimeout(showNext, duration);
    };
    const clearAll = () => { pending.length = 0; hideCurrent(); };
    clearActive.current = clearAll;
    void listen<unknown>('island-message', event => {
      const item = validMessage(event.payload);
      if (!item || disposed) return;
      const now = Date.now();
      for (const [k, until] of recentShown) {
        if (now >= until) recentShown.delete(k);
      }
      const key = `${item.source}:${item.text}`;
      const until = recentShown.get(key);
      if ((current && current.source === item.source && current.text === item.text)
        || pending.some(queued => queued.source === item.source && queued.text === item.text)
        || (until !== undefined && now < until)) {
        return;
      }
      if (pending.length >= 8) pending.shift();
      pending.push(item);
      if (!current) showNext();
    }).then(remove => { if (disposed) remove(); else unsubscribers.push(remove); }).catch(() => {});
    void listen('island-message-clear', () => {
      recentShown.clear();
      clearAll();
    }).then(remove => { if (disposed) remove(); else unsubscribers.push(remove); }).catch(() => {});
    return () => { disposed = true; clearActive.current = null; clearTimeout(timer); pending.length = 0; unsubscribers.forEach(remove => remove()); recentShown.clear(); };
  }, []);
  return message;
}
