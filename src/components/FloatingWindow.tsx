import { useEffect, useRef, useState, type ReactNode } from 'react';
import { flushSync } from 'react-dom';
import { createOrbController } from '../orb-controller';
import { resizeFloatingWindow, resizeFloatingMusic, restoreFloatingWindow, dragFloatingWindow, subscribePosition } from '../floating-window';
import { nativeOrbVisualState } from './FloatingOrb';
import { FloatingIsland } from './FloatingIsland';
import type { CoreState } from '../core-state';
import { subscribeNativeOrbEvents, updateNativeOrbVisualState } from '../native-orb';
import { listen } from '@tauri-apps/api/event';
import { changeSettings, currentSettings, useAppSettings } from '../app-settings';
import { useIslandMessages } from '../island-messages';
import { useIslandLyrics, useQqMusicMedia, useQqMusicOpen } from '../island-lyrics';
import '../preference-overrides.css';

export function FloatingWindow({ state, loading, bridgeUnavailable, children }: {
  state: CoreState | null; loading: boolean; bridgeUnavailable: boolean;
  children: (pinned: boolean, togglePin: () => void) => ReactNode;
}) {
  const [expanded, setExpanded] = useState(false);
  const [transitioning, setTransitioning] = useState(false);
  const [musicWide, setMusicWide] = useState(false);
  const [musicClosing, setMusicClosing] = useState(false);
  const { settings } = useAppSettings();
  const message = useIslandMessages(expanded);
  const lyric = useIslandLyrics();
  const musicMedia = useQqMusicMedia();
  const requestedWide = useQqMusicOpen();
  const pinned = settings?.alwaysOnTop ?? true;
  const [windowError, setWindowError] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const controller = useRef<ReturnType<typeof createOrbController> | null>(null);
  const layout = useRef<((task: () => Promise<void>, hide: () => boolean) => Promise<void>) | null>(null);
  const expandedRef = useRef(false);
  const requestedWideRef = useRef(false);
  const keyboard = useRef(false);
  const hovered = useRef(false);
  const dragging = useRef(false);
  const pointer = useRef<{ x: number; y: number } | null>(null);
  const returnFocus = useRef(false);
  const lastClick = useRef<{ x: number; y: number; time: number } | null>(null);

  useEffect(() => {
    let disposed = false;
    let unsubscribe: (() => void) | undefined;
    let unsubscribeNative: (() => void) | undefined;
    let unsubscribeCollapse: (() => void) | undefined;
    let unsubscribeTrayOpen: (() => void) | undefined;
    void subscribePosition(() => { if (!disposed) setWindowError(true); }).then(remove => {
      if (disposed) remove(); else unsubscribe = remove;
    }).catch(() => { if (!disposed) setWindowError(true); });
    const ready = restoreFloatingWindow().catch(() => { if (!disposed) setWindowError(true); });
    let pendingLayout = Promise.resolve();
    const queueLayout = (task: () => Promise<void>, hide: () => boolean) => {
      const run = pendingLayout.then(async () => {
        await ready;
        if (disposed) return;
        const hidden = hide();
        if (hidden) {
          // Hide the old view while native resizing and repositioning happen separately.
          flushSync(() => setTransitioning(true));
          await new Promise<void>(resolve => {
            const timeout = window.setTimeout(resolve, 100);
            requestAnimationFrame(() => requestAnimationFrame(() => {
              window.clearTimeout(timeout);
              resolve();
            }));
          });
        }
        if (disposed) return;
        try { await task(); }
        finally { if (hidden && !disposed) flushSync(() => setTransitioning(false)); }
      });
      pendingLayout = run.catch(() => {});
      return run;
    };
    layout.current = queueLayout;
    const c = createOrbController(next => queueLayout(async () => {
      await resizeFloatingWindow(next);
      if (!disposed) flushSync(() => { expandedRef.current = next; setExpanded(next); });
    }, () => true), () => { if (!disposed) setWindowError(false); }, () => { if (!disposed) setWindowError(true); });
    controller.current = c;
    c.setPinned(currentSettings()?.alwaysOnTop ?? true);
    void listen('floating-collapse', () => c.request(false)).then(remove => {
      if (disposed) remove(); else unsubscribeCollapse = remove;
    }).catch(() => {});
    void listen('tray-open-panel', () => c.request(true)).then(remove => {
      if (disposed) remove(); else unsubscribeTrayOpen = remove;
    }).catch(() => {});
    void subscribeNativeOrbEvents(
      () => { if (currentSettings()?.mainPanelEnabled !== false) c.request(true); },
      () => { if (!disposed) setWindowError(true); },
    ).then(remove => {
      if (disposed) remove(); else unsubscribeNative = remove;
    }).catch(() => { /* Legacy Orb remains available. */ });
    const escape = (event: KeyboardEvent) => {
      keyboard.current = true;
      if (event.key !== 'Escape') c.cancelPending();
      if (event.key !== 'Escape') return;
      event.preventDefault(); keyboard.current = true; returnFocus.current = true;
      c.escape(); c.setPinned(currentSettings()?.alwaysOnTop ?? true);
    };
    window.addEventListener('keydown', escape);
    return () => { disposed = true; c.dispose(); unsubscribe?.(); unsubscribeNative?.(); unsubscribeCollapse?.(); unsubscribeTrayOpen?.(); controller.current = null; layout.current = null; window.removeEventListener('keydown', escape); };
  }, []);
  requestedWideRef.current = requestedWide;
  useEffect(() => {
    if (requestedWide === musicWide || !layout.current) return;
    const desired = requestedWide;
    void layout.current(async () => {
      if (requestedWideRef.current !== desired) return;
      if (desired) {
        let hiddenForSlowResize = false;
        const slowTimer = !expandedRef.current ? window.setTimeout(() => {
          hiddenForSlowResize = true;
          flushSync(() => setTransitioning(true));
        }, 12) : undefined;
        try {
          await resizeFloatingMusic(true);
        } finally {
          if (slowTimer !== undefined) window.clearTimeout(slowTimer);
          if (hiddenForSlowResize) flushSync(() => setTransitioning(false));
        }
        if (requestedWideRef.current !== true) return;
        flushSync(() => {
          setMusicClosing(false);
          setMusicWide(true);
        });
      } else {
        if (!expandedRef.current && !(currentSettings()?.reduceMotion)) {
          flushSync(() => setMusicClosing(true));
          await new Promise<void>(resolve => window.setTimeout(resolve, 370));
          if (requestedWideRef.current !== false) {
            flushSync(() => setMusicClosing(false));
            return;
          }
        }
        await resizeFloatingMusic(false);
        flushSync(() => {
          setMusicClosing(false);
          setMusicWide(false);
        });
      }
      setWindowError(false);
    }, () => false).catch(() => setWindowError(true));
  }, [requestedWide, musicWide]);
  useEffect(() => {
    const visual = nativeOrbVisualState(state, loading, bridgeUnavailable, expanded);
    // Preferences suppress quota-color notifications, never transport/restricted/stale errors.
    const quotaOnly = state && state.connection === 'ready' && !loading && !state.error && !state.stale
      && state.ordinaryUsageAllowed !== false && !bridgeUnavailable
      && (visual.health === 'warning' || visual.health === 'critical');
    const suppress = quotaOnly && settings && (!settings.quotaAlertsEnabled
      || (visual.health === 'warning' && !settings.warningEnabled)
      || (visual.health === 'critical' && !settings.criticalEnabled));
    void updateNativeOrbVisualState({ ...visual, health: suppress ? 'healthy' : visual.health })
      .catch(() => { /* Native failure is surfaced by fallback; legacy stays rendered. */ });
  }, [state, loading, bridgeUnavailable, expanded, settings]);
  const panelEnabled = settings?.mainPanelEnabled !== false;
  useEffect(() => { controller.current?.setPinned(pinned); }, [pinned]);
  useEffect(() => {
    if (!panelEnabled) {
      controller.current?.cancelPending();
      if (expandedRef.current) controller.current?.request(false);
    }
  }, [panelEnabled]);
  const togglePin = () => { void changeSettings({ alwaysOnTop: !pinned }).catch(() => setWindowError(true)); };
  useEffect(() => {
    if (keyboard.current || returnFocus.current) {
      root.current?.querySelector<HTMLElement>(expanded ? '.refresh-button' : '.floating-island')?.focus();
      returnFocus.current = false;
    }
  }, [expanded]);

  return <div ref={root} data-reduce-motion={settings ? String(settings.reduceMotion) : undefined} className={`floating-surface ${expanded ? 'is-expanded' : 'is-collapsed'} ${transitioning ? 'is-transitioning' : ''} ${settings?.panelOpeningAnimation === false || settings?.reduceMotion ? 'flow-opening-off' : ''}`}
    onPointerEnter={event => { hovered.current = true; if (event.buttons === 0) dragging.current = false; if (panelEnabled && !dragging.current && !pointer.current && (expanded || settings?.orbHoverReveal !== false)) controller.current?.enter(); }}
    onPointerLeave={event => { hovered.current = false; if (event.buttons === 0) dragging.current = false; if (!keyboard.current && !dragging.current) controller.current?.leave(); }}
    onKeyDown={event => {
      keyboard.current = true;
      if (event.key !== 'Escape') controller.current?.cancelPending();
    }}
    onBlur={event => {
      if (!event.currentTarget.contains(event.relatedTarget as Node | null)) {
        keyboard.current = false;
        if (!hovered.current) controller.current?.leave();
      }
    }}
    onPointerDown={event => {
      keyboard.current = false;
      if (event.button !== 0 || (event.target as HTMLElement).closest('.pin-button, .refresh-button') || (!expanded && !(event.target as HTMLElement).closest('.floating-island, .floating-orb')) || (expanded && !(event.target as HTMLElement).closest('[data-flow-drag-region]'))) return;
      controller.current?.cancelPending();
      pointer.current = { x: event.screenX, y: event.screenY };
    }}
    onPointerMove={event => {
      if (dragging.current) {
        if (event.buttons === 0) dragging.current = false;
        return;
      }
      const start = pointer.current;
      if (!start || Math.hypot(event.screenX - start.x, event.screenY - start.y) < 5) return;
      pointer.current = null; dragging.current = true;
      lastClick.current = null;
      controller.current?.cancelPending();
      void dragFloatingWindow().catch(() => { dragging.current = false; setWindowError(true); });
    }}
    onPointerUp={event => {
      pointer.current = null;
      if (dragging.current) { dragging.current = false; return; }
      if (event.button !== 0 || (event.target as HTMLElement).closest('.pin-button, .refresh-button') || (!expanded && !(event.target as HTMLElement).closest('.floating-island, .floating-orb'))) return;
      const previous = lastClick.current;
      if (previous && event.timeStamp - previous.time < 500 && Math.hypot(event.screenX - previous.x, event.screenY - previous.y) < 5) {
        lastClick.current = null;
        togglePin();
      } else {
        lastClick.current = { x: event.screenX, y: event.screenY, time: event.timeStamp };
        if (panelEnabled && !expanded) controller.current?.request(true);
      }
    }}
    onPointerCancel={() => { pointer.current = null; }}>
    {expanded ? <>
      {children(pinned, togglePin)}
      {windowError && <span className="floating-hint" role="status">窗口操作失败，请重试</span>}
    </> : <FloatingIsland state={state} loading={loading} bridgeUnavailable={bridgeUnavailable} settings={settings} message={message} lyric={lyric} musicWide={musicWide}
      musicClosing={musicClosing} musicCover={musicMedia?.cover} musicPlaying={Boolean(lyric ? lyric.playing : musicMedia?.playing)}
      windowError={windowError} onOpen={() => { if (!panelEnabled) return; keyboard.current = true; controller.current?.request(true); }} />}
  </div>;
}
