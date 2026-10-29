import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { useAppSettings } from "../app-settings";

interface RefreshActionProps {
  refreshing: boolean;
  error?: string;
  onRefresh: () => void | Promise<void>;
  children: ReactNode;
}

// Presentation only. The caller owns the one production refresh request and
// error state. A settled request finishes its current 720ms turn, never snaps.
export function RefreshAction({ refreshing, error, onRefresh, children }: RefreshActionProps) {
  const { settings } = useAppSettings();
  const [systemReduced, setSystemReduced] = useState(() => typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  // Explicit user opt-in: this button follows the app's motion switch, even
  // when Windows animations are off. Use the OS preference only while loading.
  const reduced = settings?.reduceMotion ?? systemReduced;
  const [request, setRequest] = useState<"idle" | "pending" | "settled">("idle");
  const [rotating, setRotating] = useState(false);
  const [result, setResult] = useState<"success" | "error" | "">("");
  const [tooltipDismissed, setTooltipDismissed] = useState(false);
  const rejected = useRef(false);
  const locked = useRef(false);
  const mounted = useRef(true);
  const tooltipId = useId();
  const busy = refreshing || request === "pending";

  useEffect(() => {
    mounted.current = true;
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    const changed = () => setSystemReduced(media.matches);
    changed();
    media.addEventListener("change", changed);
    return () => {
      mounted.current = false;
      media.removeEventListener("change", changed);
    };
  }, []);

  function finish() {
    setRotating(false);
    setRequest("idle");
    setResult(rejected.current || error ? "error" : "success");
    locked.current = false;
  }

  useEffect(() => {
    if (busy) {
      locked.current = true;
      setRotating(true);
      setResult("");
    } else if (reduced && rotating) {
      // Static busy feedback has no animationiteration event to wait for.
      finish();
    }
  }, [busy, reduced, rotating, error]);

  useEffect(() => {
    if (!result) return;
    const timer = window.setTimeout(() => setResult(""), result === "error" ? 200 : 160);
    return () => window.clearTimeout(timer);
  }, [result]);

  async function refresh() {
    if (locked.current || busy || rotating) return;
    locked.current = true; // Also guards two clicks before React commits.
    rejected.current = false;
    setRequest("pending");
    setRotating(true);
    setResult("");
    try {
      await onRefresh();
    } catch {
      rejected.current = true;
    } finally {
      if (mounted.current) setRequest("settled");
    }
  }

  return (
    <button
      className="refresh-button character-refresh refresh-action"
      type="button"
      data-cat-target="refresh"
      onClick={() => void refresh()}
      onPointerEnter={() => {
        setTooltipDismissed(false);
      }}
      onFocus={() => setTooltipDismissed(false)}
      onKeyDown={event => {
        if (event.key === "Escape" && !tooltipDismissed) {
          setTooltipDismissed(true);
          event.stopPropagation();
        }
      }}
      aria-label={busy ? "正在刷新额度" : "刷新额度"}
      aria-busy={busy}
      aria-disabled={busy || rotating}
      aria-describedby={tooltipId}
      data-refreshing={busy}
      data-animating={rotating}
      data-result={result}
      data-reduced-motion={reduced}
      data-tooltip-dismissed={tooltipDismissed}
    >
      <span className="refresh-icon-clip" aria-hidden="true">
        <span className="refresh-icon-enter">
          <span className="refresh-icon-rotor" onAnimationIteration={() => { if (!busy) finish(); }}>
            {children}
          </span>
        </span>
      </span>
      <span className="refresh-tooltip" id={tooltipId} role="tooltip">刷新额度</span>
      <span className="refresh-announcement" role="status">
        {result === "error" ? (error ?? "刷新失败，请稍后重试") : result === "success" ? "刷新完成" : ""}
      </span>
    </button>
  );
}
