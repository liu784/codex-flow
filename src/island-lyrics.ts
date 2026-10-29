import { useEffect, useState } from 'react';
import { isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export type LyricWord = { text: string; startMs: number; durationMs: number };
export type IslandLyric = { lineStartMs: number; positionMs: number; playing: boolean; words: LyricWord[]; receivedAt: number; cover?: string | null };
export type QqMusicMedia = { cover: string | null; playing: boolean };

const COVER_DATA_URL_RE = /^data:image\/(?:png|jpeg|webp|gif|bmp);base64,[A-Za-z0-9+/]+=*$/;

function validCover(value: unknown): string | null {
  return typeof value === 'string' && value.length <= 3_000_000 && COVER_DATA_URL_RE.test(value) ? value : null;
}

function validLyric(value: unknown): IslandLyric | null {
  if (!value || typeof value !== 'object') return null;
  const item = value as Record<string, unknown>;
  if (!Number.isFinite(item.lineStartMs) || !Number.isFinite(item.positionMs) || typeof item.playing !== 'boolean' || !Array.isArray(item.words) || item.words.length < 1 || item.words.length > 200) return null;
  const words: LyricWord[] = [];
  for (const value of item.words) {
    if (!value || typeof value !== 'object') return null;
    const word = value as Record<string, unknown>;
    if (typeof word.text !== 'string' || word.text.length > 500 || !Number.isFinite(word.startMs) || !Number.isFinite(word.durationMs) || (word.durationMs as number) <= 0) return null;
    words.push({ text: word.text, startMs: word.startMs as number, durationMs: word.durationMs as number });
  }
  const cover = validCover(item.cover);
  return { lineStartMs: item.lineStartMs as number, positionMs: item.positionMs as number, playing: item.playing, words, receivedAt: performance.now(), ...(cover ? { cover } : {}) };
}

function validMedia(value: unknown): QqMusicMedia | null {
  if (!value || typeof value !== 'object') return null;
  const item = value as Record<string, unknown>;
  if (typeof item.playing !== 'boolean') return null;
  return { cover: validCover(item.cover), playing: item.playing };
}

export function useIslandLyrics() {
  const [lyric, setLyric] = useState<IslandLyric | null>(null);
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let unsubscribe: (() => void) | undefined;
    void listen<unknown>('island-lyric', event => {
      if (!disposed) setLyric(validLyric(event.payload));
    }).then(remove => { if (disposed) remove(); else unsubscribe = remove; }).catch(() => {});
    return () => { disposed = true; unsubscribe?.(); };
  }, []);
  return lyric;
}

export function useQqMusicMedia() {
  const [media, setMedia] = useState<QqMusicMedia | null>(null);
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let unsubscribe: (() => void) | undefined;
    void listen<unknown>('qq-music-media', event => {
      if (disposed) return;
      const next = validMedia(event.payload);
      setMedia(prev => (prev && next && prev.cover === next.cover && prev.playing === next.playing) ? prev : next);
    }).then(remove => { if (disposed) remove(); else unsubscribe = remove; }).catch(() => {});
    return () => { disposed = true; unsubscribe?.(); };
  }, []);
  return media;
}

export function useQqMusicOpen() {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!isTauri()) return;
    let disposed = false;
    let unsubscribe: (() => void) | undefined;
    void listen<unknown>('qq-music-open', event => {
      if (!disposed && typeof event.payload === 'boolean') setOpen(event.payload);
    }).then(remove => { if (disposed) remove(); else unsubscribe = remove; }).catch(() => {});
    return () => { disposed = true; unsubscribe?.(); };
  }, []);
  return open;
}
