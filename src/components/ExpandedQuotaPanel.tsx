import { useLayoutEffect, useRef, useMemo } from "react";
import { CatController } from '../cat-controller';
import { useAppSettings } from '../app-settings';
import {
  formatPlan,
  formatResetTime,
  getErrorMessage,
  getRemainingPercent,
  selectDisplayQuotaWindows,
  statusTone,
  type CoreState,
} from "../core-state";
import { CharacterPortrait } from "./CharacterPortrait";
import { RefreshAction } from "./RefreshAction";

interface ExpandedQuotaPanelProps {
  state: CoreState | null;
  loading: boolean;
  refreshing: boolean;
  bridgeUnavailable: boolean;
  onRefresh: () => void | Promise<void>;
  pinned?: boolean;
  onTogglePin?: () => void;
  onSettings?: () => void;
}

function PinIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M7 4h10M8 4v5l-4 4v2h16v-2l-4-4V4M12 15v5" />
    </svg>
  );
}

function SettingsIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <circle cx="12" cy="12" r="3" />
      <path d="m10 3-.6 2.4-2 .9-2.2-.7-2 3.4 1.7 1.7v2.6L3.2 15l2 3.4 2.2-.7 2 .9.6 2.4h4l.6-2.4 2-.9 2.2.7 2-3.4-1.7-1.7v-2.6L20.8 9l-2-3.4-2.2.7-2-.9L14 3Z" />
    </svg>
  );
}

function ActionLabels({ normal, hover }: { normal: string; hover: string }) {
  return (
    <span className="action-labels">
      <span className="label-normal">{normal}</span>
      <span className="label-hover" aria-hidden="true">{hover}</span>
    </span>
  );
}

function RefreshIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M5 9a7.5 7.5 0 0 1 12.3-2.3L19 8.5M19 5v3.5h-3.5M19 15a7.5 7.5 0 0 1-12.3 2.3L5 15.5M5 19v-3.5h3.5" />
    </svg>
  );
}

function MetricIcon({ kind }: { kind: "quota" | "reset" | "time" }) {
  if (kind === "quota") {
    return (
      <svg viewBox="0 0 24 24" aria-hidden="true" data-symbol="honeycomb-nodes">
        <path d="m12 3 2.6 1.5v3L12 9l-2.6-1.5v-3Zm0 6 2.6 1.5v3L12 15l-2.6-1.5v-3Zm0 6 2.6 1.5v3L12 21l-2.6-1.5v-3M6.8 6l2.6 1.5v3L6.8 12l-2.6-1.5v-3Zm0 6 2.6 1.5v3L6.8 18l-2.6-1.5v-3M17.2 6l2.6 1.5v3L17.2 12l-2.6-1.5v-3Zm0 6 2.6 1.5v3L17.2 18l-2.6-1.5v-3" />
      </svg>
    );
  }
  if (kind === "reset") {
    return <svg viewBox="0 0 24 24" aria-hidden="true" data-symbol="double-rings"><circle cx="12" cy="8.5" r="6.5" /><circle cx="12" cy="15.5" r="6.5" /></svg>;
  }
  return <svg viewBox="0 0 24 24" aria-hidden="true" data-symbol="cylinder"><ellipse cx="12" cy="5.5" rx="8" ry="3" /><path d="M4 5.5v13c0 1.66 3.58 3 8 3s8-1.34 8-3v-13M4 16c0 1.66 3.58 3 8 3s8-1.34 8-3" /></svg>;
}

function resetMetricValue(resetsAt: number | null): string {
  const formatted = formatResetTime(resetsAt);
  if (formatted === "重置时间不可用") return "--";
  if (formatted === "即将重置") return "即将";
  return formatted.replace(/(?:后)?重置$/, "").trim();
}

function Metric({
  kind,
  value,
  label,
}: {
  kind: "quota" | "reset" | "time";
  value: string;
  label: string;
}) {
  return (
    <div className={`panel-metric metric-${kind}`} data-cat-target={kind === 'quota' ? 'quota' : kind === 'reset' ? 'reset-count' : 'next-reset'}>
      <span className="metric-icon"><MetricIcon kind={kind} /></span>
      <span className="metric-copy">
        <MetricValue value={value} animate={kind !== "time"} />
        <span>{label}</span>
      </span>
    </div>
  );
}

// Opening-only presentation. Never writes back into quota state, and never
// animates a date. A later live refresh displays its new value immediately.
function MetricValue({ value, animate }: { value: string; animate: boolean }) {
  const { settings } = useAppSettings();
  const openingEnabled = settings?.panelOpeningAnimation !== false && settings?.reduceMotion !== true;
  const textRef = useRef<HTMLElement>(null);
  const played = useRef(false);
  useLayoutEffect(() => {
    const text = textRef.current;
    if (!text) return;
    text.textContent = value;
    text.setAttribute("aria-label", value);
    const match = /^(\d+)(%|次)$/.exec(value);
    if (!animate || !openingEnabled || !match || played.current) return;
    // User explicitly opted these two opening effects into playback even when
    // Windows client-area animations are disabled. No OS preference is changed.
    const target = Number(match[1]);
    const suffix = match[2];
    const started = performance.now();
    let frame = 0;
    text.textContent = `0${suffix}`;
    const finish = () => {
      cancelAnimationFrame(frame);
      text.textContent = value;
      played.current = true;
    };
    const tick = (now: number) => {
      const phase = Math.min(1, Math.max(0, (now - started - 140) / 1000));
      const eased = 1 - (1 - phase) ** 3;
      text.textContent = `${Math.floor(target * eased)}${suffix}`;
      if (phase < 1) frame = requestAnimationFrame(tick);
      else finish();
    };
    frame = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(frame);
    };
  }, [value, animate, openingEnabled]);

  return <strong ref={textRef}>{value}</strong>;
}

export function ExpandedQuotaPanel({
  state,
  loading,
  refreshing,
  bridgeUnavailable,
  onRefresh,
  pinned = true,
  onTogglePin,
  onSettings,
}: ExpandedQuotaPanelProps) {
  const cat = useMemo(() => new CatController(), []);
  const { settings } = useAppSettings();
  const systemReduced = typeof window !== "undefined"
    && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  // The explicit in-app switch owns these requested effects once loaded.
  const actionReduced = settings?.reduceMotion ?? systemReduced;
  const { primary } = selectDisplayQuotaWindows(state?.windows ?? [], state?.account ?? null);
  const remaining = getRemainingPercent(primary);
  const tone = statusTone(state, remaining);
  const visibleError = getErrorMessage(state?.error ?? null)
    ?? (bridgeUnavailable ? { title: "无法连接应用核心" } : null);
  const quotaValue = loading || remaining === null ? "--" : `${remaining}%`;
  const resetCredits = state?.resetCreditsAvailable;
  const resetCountValue = resetCredits === null || resetCredits === undefined ? "--" : `${resetCredits}次`;
  const resetTimeValue = resetMetricValue(primary?.resetsAt ?? null);

  return (
    <section
      className={`quota-panel character-panel glass tone-${tone}`}
      aria-labelledby="quota-title"
      aria-busy={loading || refreshing}
    >
      <header className="panel-header character-panel-header" data-flow-drag-region>
        <span className="panel-drag-area" aria-hidden="true" data-flow-drag-region />
        <div className="panel-utilities">
          <button
            type="button"
            className={`utility-icon glass-action pin-action pin-button ${pinned ? "is-active" : ""}`}
            title={pinned ? "取消固定展开" : "固定展开"}
            aria-label={pinned ? "取消固定展开" : "固定展开"}
            aria-pressed={pinned}
            data-reduced-motion={actionReduced}
            onClick={onTogglePin}
            data-cat-target="pin"
          >
            <span className="action-icon"><PinIcon /></span>
            <ActionLabels
              normal={pinned ? "已固定" : "固定"}
              hover={pinned ? "取消固定" : "吸附置顶"}
            />
          </button>
          <button
            className="utility-icon glass-action settings-action panel-settings-button"
            type="button"
            title="设置"
            aria-label="设置"
            data-reduced-motion={actionReduced}
            disabled={!onSettings}
            onClick={onSettings}
            data-cat-target="settings"
          >
            <span className="action-icon"><SettingsIcon /></span>
            <ActionLabels normal="设置" hover="打开设置" />
          </button>
        </div>
      </header>

      <div className="character-stage">
        <div className="character-hero" data-flow-drag-region>
          <h1 id="quota-title" className="package-title">{formatPlan(state?.account ?? null)}</h1>
          <CharacterPortrait controller={cat} />
        </div>

        <aside className="panel-metrics" aria-label="额度信息">
          <Metric kind="quota" value={quotaValue} label="剩余额度" />
          <Metric kind="reset" value={resetCountValue} label="可用重置" />
          <Metric kind="time" value={resetTimeValue} label="下次重置" />
        </aside>
      </div>

      <RefreshAction refreshing={refreshing} error={visibleError?.title} onRefresh={onRefresh}>
        <RefreshIcon />
      </RefreshAction>
    </section>
  );
}
