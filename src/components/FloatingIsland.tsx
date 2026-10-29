import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from 'react';
import type { AppSettings } from '../app-settings';
import { selectDisplayQuotaWindows, type CoreState } from '../core-state';
import { DEFAULT_ISLAND_AVATAR, useIslandAvatar } from '../island-avatar';
import { messageDisplayDuration, type IslandMessage } from '../island-messages';
import type { IslandLyric } from '../island-lyrics';
import { orbPresentation } from './FloatingOrb';
import './floating-island.css';

function IslandMessageView({ message }: { message: IslandMessage }) {
  const frame = useRef<HTMLSpanElement>(null);
  const content = useRef<HTMLSpanElement>(null);
  const [distance, setDistance] = useState(0);
  useLayoutEffect(() => {
    const frameElement = frame.current;
    const contentElement = content.current;
    if (!frameElement || !contentElement) return;
    const measure = () => setDistance(Math.max(0, Math.ceil(Math.max(contentElement.scrollWidth, contentElement.getBoundingClientRect().width) - frameElement.clientWidth)));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(frameElement);
    observer.observe(contentElement);
    return () => observer.disconnect();
  }, [message.text]);
  const totalDuration = messageDisplayDuration(message.text);
  const animDuration = Math.max(4500, totalDuration - 1500);
  const style = { '--message-distance': `${distance}px`, '--message-steps': Math.max(1, distance), '--message-duration': `${animDuration}ms` } as CSSProperties;
  return <span className="island-message" aria-live="polite" style={style}>
    <span className="island-message-source">{message.source}</span>
    <span className={`island-message-copy${distance > 1 ? ' is-scrolling' : ''}`} ref={frame}>
      <span ref={content}>{message.text}</span>
    </span>
  </span>;
}

function IslandLyricView({ lyric, reduceMotion }: { lyric: IslandLyric; reduceMotion: boolean }) {
  const frame = useRef<HTMLSpanElement>(null);
  const track = useRef<HTMLSpanElement>(null);
  const scrollTarget = useRef(0);
  const scrollPosition = useRef(0);
  const timing = useRef<{ key: string; position: number; at: number; playing: boolean; correction: number } | null>(null);
  const lineText = lyric.words.map(word => word.text).join('');
  const lineKey = `${lyric.lineStartMs}:${lineText}`;
  const previous = useRef<{ key: string; text: string; scroll: number } | null>(null);
  const [outgoing, setOutgoing] = useState<{ key: string; text: string; scroll: number } | null>(null);
  const [clock, setClock] = useState({ key: lineKey, position: lyric.positionMs });
  const position = clock.key === lineKey ? clock.position : lyric.positionMs;
  const characters = lyric.words.flatMap(word => {
    const characters = Array.from(word.text);
    return characters.map((text, index) => ({ text, startMs: word.startMs + word.durationMs * index / characters.length, durationMs: word.durationMs / characters.length }));
  });
  useEffect(() => {
    let animation = 0;
    const now = performance.now();
    const sample = lyric.positionMs + (lyric.playing ? Math.max(0, Math.min(1500, now - lyric.receivedAt)) : 0);
    const prior = timing.current;
    const priorElapsed = prior?.playing ? Math.max(0, Math.min(1500, now - prior.at)) : 0;
    const predicted = prior ? prior.position + priorElapsed + prior.correction * Math.min(1, priorElapsed / 500) : sample;
    const error = sample - predicted;
    const smoothCorrection = prior?.key === lineKey && prior.playing && lyric.playing && Math.abs(error) <= 250;
    const anchor = { key: lineKey, position: smoothCorrection ? predicted : sample, at: now, playing: lyric.playing, correction: smoothCorrection ? error : 0 };
    timing.current = anchor;
    const update = () => {
      const elapsed = anchor.playing ? Math.max(0, Math.min(1500, performance.now() - anchor.at)) : 0;
      setClock({ key: lineKey, position: anchor.position + elapsed + anchor.correction * Math.min(1, elapsed / 500) });
      if (previous.current?.key === lineKey) previous.current.scroll = scrollPosition.current;
      if (track.current && !reduceMotion) {
        const difference = scrollTarget.current - scrollPosition.current;
        scrollPosition.current += difference * .16;
        track.current.style.transform = `translate3d(${-scrollPosition.current}px, 0, 0)`;
      }
      if (lyric.playing) animation = requestAnimationFrame(update);
    };
    update();
    return () => cancelAnimationFrame(animation);
  }, [lyric, lineKey, reduceMotion]);
  const nextWord = characters.findIndex(word => position < word.startMs + word.durationMs);
  const active = nextWord < 0 ? characters.length - 1 : nextWord;
  useLayoutEffect(() => {
    const old = previous.current;
    if (old && old.key !== lineKey && !reduceMotion) setOutgoing(old);
    else setOutgoing(null);
    previous.current = { key: lineKey, text: lineText, scroll: 0 };
    scrollTarget.current = 0;
    scrollPosition.current = 0;
    if (track.current) track.current.style.transform = 'translate3d(0, 0, 0)';
    const timer = setTimeout(() => setOutgoing(null), 320);
    return () => clearTimeout(timer);
  }, [lineKey, lineText, reduceMotion]);
  useLayoutEffect(() => {
    const element = frame.current;
    if (!element) return;
    const word = element.querySelector<HTMLElement>(`[data-lyric-word="${active}"]`);
    const activeCharacter = characters[active];
    if (!word || !activeCharacter || !track.current) return;
    const left = word.getBoundingClientRect().left - element.getBoundingClientRect().left + scrollPosition.current;
    const progress = Math.max(0, Math.min(1, (position - activeCharacter.startMs) / activeCharacter.durationMs));
    const focus = left + word.getBoundingClientRect().width * progress;
    const maxScroll = Math.max(0, track.current.getBoundingClientRect().width - element.clientWidth);
    scrollTarget.current = Math.max(0, Math.min(maxScroll, focus - element.clientWidth * .55));
    if (reduceMotion || !lyric.playing) {
      scrollPosition.current = scrollTarget.current;
      track.current.style.transform = `translate3d(${-scrollPosition.current}px, 0, 0)`;
    }
  }, [active, lineKey, lyric.playing, position, reduceMotion]);
  return <span className="island-message island-lyric" aria-label={`QQ 音乐 ${lineText}`}>
    <span className="island-message-source">QQ 音乐</span>
    <span className="island-lyric-stage" aria-hidden="true">
    {outgoing && <span className="island-lyric-outgoing" key={outgoing.key}><span className="island-lyric-line" style={{ transform: `translate3d(${-outgoing.scroll}px, 0, 0)` }}>{outgoing.text}</span></span>}
    <span className="island-lyric-copy" key={lineKey} ref={frame}>
      <span className="island-lyric-line" ref={track}>
      {characters.map((word, index) => {
        const fill = Math.max(0, Math.min(100, (position - word.startMs) / word.durationMs * 100));
        const progress = fill / 100;
        const glow = fill <= 0 ? 0 : fill >= 100 ? .32 : .32 * progress + .64 * Math.sin(Math.PI * progress);
        const feather = 12 * Math.sin(Math.PI * progress);
        return <span key={`${index}-${word.startMs}`} data-lyric-word={index} className={fill <= 0 ? 'is-waiting' : fill < 100 ? 'is-active' : 'is-passed'}
          style={{ '--lyric-fill': `${fill}%`, '--lyric-color-start': `${Math.max(0, fill - feather)}%`, '--lyric-color-end': `${Math.min(100, fill + feather)}%`, '--lyric-glow': glow.toFixed(3) } as CSSProperties}>{word.text}</span>;
      })}
      </span>
    </span>
    </span>
  </span>;
}

function getPrimaryResetAt(state: CoreState | null): number | null {
  const primary = selectDisplayQuotaWindows(state?.windows ?? [], state?.account ?? null).primary;
  const ts = primary?.resetsAt ?? null;
  return ts !== null && Number.isFinite(ts) ? ts : null;
}

const DAY_WHISPERS = [
  '今天也在努力呀~',
  '喝口水歇一下喵',
  '摸摸头不着急喵',
  '保持好心情喵~',
  '随时待命陪你喵',
];

const NIGHT_WHISPERS = [
  '今晚也在努力呀~',
  '喝口水歇一下喵',
  '摸摸头不着急喵',
  '眨眨眼放松下喵',
  '随时待命陪你喵',
];

function IslandIdleCompanion({ state, reduceMotion }: { state: CoreState | null; reduceMotion: boolean }) {
  const [step, setStep] = useState(0);
  const [nowMs, setNowMs] = useState(() => Date.now());
  useEffect(() => {
    const rotateTimer = window.setInterval(() => {
      setNowMs(Date.now());
      setStep(n => n + 1);
    }, 45000);
    const clockTimer = window.setInterval(() => {
      setNowMs(Date.now());
    }, 15000);
    return () => {
      window.clearInterval(rotateTimer);
      window.clearInterval(clockTimer);
    };
  }, []);

  const now = new Date(nowMs);
  const hours = now.getHours();
  const minutes = now.getMinutes();
  const timeStr = `${String(hours).padStart(2, '0')}:${String(minutes).padStart(2, '0')}`;
  const isLateNight = hours >= 22 || hours < 5;
  const isEvening = hours >= 18 && hours < 22;
  const mode = step % 3;

  if (mode === 0) {
    let label = '专注中';
    let sub = '状态拉满喵 ~';
    let iconKind: 'moon' | 'sun' | 'paw' = 'paw';
    if (isLateNight) {
      label = '夜深了';
      sub = '早点休息喵';
      iconKind = 'moon';
    } else if (hours >= 5 && hours < 9) {
      label = '早安呀';
      sub = '元气满满喵 ~';
      iconKind = 'sun';
    } else if (hours >= 12 && hours < 14) {
      label = '午休啦';
      sub = '记得干饭喵 ~';
      iconKind = 'sun';
    } else if (hours >= 14 && hours < 18) {
      label = '下午好';
      sub = '喝口水歇下喵';
      iconKind = 'paw';
    } else if (isEvening) {
      label = '晚上好';
      sub = '辛苦一天啦喵';
      iconKind = 'moon';
    }

    return <span className={`island-idle-companion${reduceMotion ? ' is-static' : ''}`} aria-hidden="true">
      <span className="island-idle-b1" key="b1">
        <span className={`island-idle-b1-top is-${iconKind}`}>
          {iconKind === 'moon' ? (
            <svg viewBox="0 0 24 24"><path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" /></svg>
          ) : iconKind === 'sun' ? (
            <svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="5" /><path d="M12 1v3M12 20v3M4.22 4.22l2.12 2.12M17.66 17.66l2.12 2.12M1 12h3M20 12h3M4.22 19.78l2.12-2.12M17.66 6.34l2.12-2.12" stroke="currentColor" strokeWidth="2" strokeLinecap="round" /></svg>
          ) : (
            <svg viewBox="0 0 24 24"><circle cx="7" cy="8" r="2.2" /><circle cx="12" cy="6" r="2.2" /><circle cx="17" cy="8" r="2.2" /><path d="M12 11c-3.5 0-6 2.4-6 5.2 0 1.8 1.5 2.8 3.3 2.8.9 0 1.8-.3 2.7-.3s1.8.3 2.7.3c1.8 0 3.3-1 3.3-2.8 0-2.8-2.5-5.2-6-5.2z" /></svg>
          )}
          <span>{timeStr} · {label}</span>
        </span>
        <span className="island-idle-b1-sub">
          <span>{sub}</span>
          {isLateNight && <span className="island-idle-zzz">zZ</span>}
        </span>
      </span>
    </span>;
  }

  if (mode === 1) {
    const resetAt = getPrimaryResetAt(state);
    let resetContent: React.ReactNode = <>重置时间未知</>;
    if (resetAt !== null) {
      const remMin = Math.max(0, Math.ceil((resetAt * 1000 - nowMs) / 60000));
      if (remMin === 0) {
        resetContent = <><span className="island-idle-hl">即将</span> 重置喵~</>;
      } else if (remMin < 60) {
        resetContent = <><span className="island-idle-hl">{remMin}m</span> 后重置喵~</>;
      } else if (remMin < 48 * 60) {
        const h = Math.floor(remMin / 60);
        const m = remMin % 60;
        resetContent = m === 0
          ? <><span className="island-idle-hl">{h}h</span> 后重置喵~</>
          : <><span className="island-idle-hl">{h}h {m}m</span> 后重置</>;
      } else {
        const d = Math.floor(remMin / 1440);
        const h = Math.floor((remMin % 1440) / 60);
        resetContent = h === 0
          ? <><span className="island-idle-hl">{d}天</span> 后重置喵~</>
          : <><span className="island-idle-hl">{d}天{h}h</span> 后重置</>;
      }
    }
    return <span className={`island-idle-companion${reduceMotion ? ' is-static' : ''}`} aria-hidden="true">
      <span className="island-idle-bubble" key="b2-reset">
        <span className="island-idle-dot is-reset" />
        <span className="island-idle-bubble-text">{resetContent}</span>
      </span>
    </span>;
  }

  const pool = isLateNight || isEvening ? NIGHT_WHISPERS : DAY_WHISPERS;
  const whisper = pool[Math.floor(step / 3) % pool.length];
  return <span className={`island-idle-companion${reduceMotion ? ' is-static' : ''}`} aria-hidden="true">
    <span className="island-idle-bubble" key={`b2-whisper-${step}`}>
      <span className="island-idle-dot" />
      <span className="island-idle-bubble-text">{whisper}</span>
    </span>
  </span>;
}

export function FloatingIsland({ state, loading, bridgeUnavailable, settings, onOpen, windowError, message, lyric, musicWide, musicClosing, musicCover, musicPlaying }: {
  state: CoreState | null; loading: boolean; bridgeUnavailable: boolean; musicWide?: boolean; musicClosing?: boolean;
  settings: AppSettings | null; onOpen: () => void; windowError: boolean; message?: IslandMessage | null; lyric?: IslandLyric | null;
  musicCover?: string | null; musicPlaying?: boolean;
}) {
  const { avatar } = useIslandAvatar();
  const view = orbPresentation(state, loading, bridgeUnavailable);
  // Preserve the existing alert preferences without suppressing connection errors.
  const quotaOnly = state && state.connection === 'ready' && !loading && !state.error && !state.stale
    && state.ordinaryUsageAllowed !== false && !bridgeUnavailable
    && (view.tone === 'warning' || view.tone === 'critical');
  const suppress = quotaOnly && settings && (!settings.quotaAlertsEnabled
    || (view.tone === 'warning' && !settings.warningEnabled)
    || (view.tone === 'critical' && !settings.criticalEnabled));
  const tone = suppress ? 'healthy' : view.tone;
  const progress = Math.max(0, Math.min(100, view.progress ?? 0));
  const style = { '--island-progress': `${progress}%` } as CSSProperties;
  const firstConnection = view.progress === null && !bridgeUnavailable && !state?.error
    && state?.ordinaryUsageAllowed !== false
    && (loading || (state?.lastSuccessAt == null
      && (state?.connection === 'connecting' || state?.connection === 'disconnected')));
  const status = firstConnection ? '正在连接' : view.status;
  const lowQuota = view.progress !== null && !suppress && !view.marker
    && (tone === 'warning' || tone === 'critical');
  const marker = view.marker === '!' ? null : view.marker;
  const alertLabel = lowQuota ? (tone === 'critical' ? '，额度严重不足' : '，额度偏低') : '';
  const albumCover = lyric?.cover ?? musicCover ?? null;
  const isPlaying = musicPlaying ?? lyric?.playing ?? Boolean(lyric);
  const showCover = Boolean(albumCover && isPlaying);

  return <button type="button" className={`floating-island tone-${tone}${musicWide ? ' is-music-wide' : ''}${musicClosing ? ' is-music-closing' : ''}`} style={style}
    aria-label={`${message ? `${message.source} ${message.text}，` : lyric ? `QQ 音乐 ${lyric.words.map(word => word.text).join('')}，` : ''}${status}${firstConnection ? '' : ` ${view.value}`}${alertLabel}，展开额度面板`}
    aria-expanded={false}
    title={windowError ? '窗口操作失败，请重试' : '点击展开，拖动调整位置'}
    onClick={event => { if (event.detail === 0) onOpen(); }}>
    <span className={`island-icon${avatar ? ' is-custom' : ''}`} aria-hidden="true"><img src={avatar ?? DEFAULT_ISLAND_AVATAR} alt="" /></span>
    {message ? <IslandMessageView key={`${message.source}:${message.text}`} message={message} /> : lyric ? <IslandLyricView lyric={lyric} reduceMotion={settings?.reduceMotion ?? false} />
      : firstConnection ? <span className="island-status">正在连接</span>
      : view.marker === '!' ? <span className="island-status">{status}</span>
      : <IslandIdleCompanion state={state} reduceMotion={settings?.reduceMotion ?? false} />}
    {!firstConnection && <span className="island-metric"><span className="island-value" aria-hidden="true">
      {view.progress === null ? view.value : Array.from(view.value).map((character, index) =>
        <span className="island-digit" key={`${index}-${character}`} style={{ animationDelay: `${index * 0.25}s` }}>{character}</span>)}
    </span>
      <span className="island-track" aria-hidden="true">{progress > 0 && <span />}</span></span>}
    {!firstConnection && marker && <span className={`island-marker ${tone === 'critical' ? 'is-critical' : ''}`} aria-hidden="true">{marker}</span>}
    {musicWide && <>
      <span className="island-ink-bridge" aria-hidden="true" />
      <span className="island-sub-pod" aria-hidden="true">
        {showCover && <span className="island-sub-cover"><img src={albumCover!} alt="" /></span>}
      </span>
    </>}
  </button>;
}
