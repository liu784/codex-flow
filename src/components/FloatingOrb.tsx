import { useId } from 'react';
import { getRemainingPercent, selectDisplayQuotaWindows, statusTone, getErrorMessage, type CoreState } from '../core-state';

const ORB_CSS_SCALE = 0.88;
export const ORB_PROGRESS_GEOMETRY = Object.freeze({
  outerRadius: 34,
  innerRadius: 27.2,
  thickness: 6.8,
  outerDiameterCssPx: 34 * 2 * ORB_CSS_SCALE,
  innerRadiusCssPx: 27.2 * ORB_CSS_SCALE,
  thicknessCssPx: 6.8 * ORB_CSS_SCALE,
});

function coordinate(radius: number, angleDegrees: number) {
  const radians = angleDegrees * Math.PI / 180;
  return { x: 50 + radius * Math.cos(radians), y: 50 + radius * Math.sin(radians) };
}

function n(value: number) {
  return Number(value.toFixed(3));
}

export function annularProgressSegment(rawProgress: number | null) {
  const progress = Math.min(100, Math.max(0, rawProgress ?? 0));
  const sweepDegrees = progress * 3.6;
  const { outerRadius, innerRadius, thickness } = ORB_PROGRESS_GEOMETRY;
  const middleRadius = (outerRadius + innerRadius) / 2;
  const capRadius = thickness / 2;

  if (progress <= 0) return { path: '', progress, sweepDegrees };
  if (progress >= 100) {
    return {
      path: `M 50 ${n(50 - outerRadius)} A ${outerRadius} ${outerRadius} 0 1 1 49.999 ${n(50 - outerRadius)} Z `
        + `M 50 ${n(50 - innerRadius)} A ${innerRadius} ${innerRadius} 0 1 0 50.001 ${n(50 - innerRadius)} Z`,
      progress,
      sweepDegrees,
    };
  }

  const capAngle = Math.asin(capRadius / middleRadius) * 180 / Math.PI;
  if (sweepDegrees <= capAngle * 2) {
    const center = coordinate(middleRadius, -90 + sweepDegrees / 2);
    const radius = middleRadius * Math.sin(sweepDegrees * Math.PI / 360);
    return {
      path: `M ${n(center.x + radius)} ${n(center.y)} A ${n(radius)} ${n(radius)} 0 1 1 ${n(center.x - radius)} ${n(center.y)} `
        + `A ${n(radius)} ${n(radius)} 0 1 1 ${n(center.x + radius)} ${n(center.y)} Z`,
      progress,
      sweepDegrees,
    };
  }

  const bodyStart = -90 + capAngle;
  const bodyEnd = -90 + sweepDegrees - capAngle;
  const bodySweep = bodyEnd - bodyStart;
  const largeArc = bodySweep > 180 ? 1 : 0;
  const outerStart = coordinate(outerRadius, bodyStart);
  const outerEnd = coordinate(outerRadius, bodyEnd);
  const innerEnd = coordinate(innerRadius, bodyEnd);
  const innerStart = coordinate(innerRadius, bodyStart);
  return {
    path: `M ${n(outerStart.x)} ${n(outerStart.y)} `
      + `A ${outerRadius} ${outerRadius} 0 ${largeArc} 1 ${n(outerEnd.x)} ${n(outerEnd.y)} `
      + `A ${capRadius} ${capRadius} 0 0 1 ${n(innerEnd.x)} ${n(innerEnd.y)} `
      + `A ${innerRadius} ${innerRadius} 0 ${largeArc} 0 ${n(innerStart.x)} ${n(innerStart.y)} `
      + `A ${capRadius} ${capRadius} 0 0 1 ${n(outerStart.x)} ${n(outerStart.y)} Z`,
    progress,
    sweepDegrees,
  };
}

export function annularProgressCore(rawProgress: number | null) {
  const progress = Math.min(100, Math.max(0, rawProgress ?? 0));
  const sweepDegrees = progress * 3.6;
  const { outerRadius, innerRadius, thickness } = ORB_PROGRESS_GEOMETRY;
  const middleRadius = (outerRadius + innerRadius) / 2;
  const capAngle = Math.asin((thickness / 2) / middleRadius) * 180 / Math.PI;

  if (progress <= 0) return '';
  if (progress >= 100) {
    return `M 50 ${n(50 - middleRadius)} A ${n(middleRadius)} ${n(middleRadius)} 0 1 1 49.999 ${n(50 - middleRadius)}`;
  }

  const bodyStart = -90 + Math.min(capAngle, sweepDegrees / 2);
  const bodyEnd = -90 + sweepDegrees - Math.min(capAngle, sweepDegrees / 2);
  const start = coordinate(middleRadius, bodyStart);
  const end = coordinate(middleRadius, bodyEnd);
  const bodySweep = Math.max(0, bodyEnd - bodyStart);
  return `M ${n(start.x)} ${n(start.y)} A ${n(middleRadius)} ${n(middleRadius)} 0 ${bodySweep > 180 ? 1 : 0} 1 ${n(end.x)} ${n(end.y)}`;
}

export function orbPresentation(state: CoreState | null, loading: boolean, bridgeUnavailable = false) {
  const remaining = getRemainingPercent(selectDisplayQuotaWindows(
    state?.windows ?? [],
    state?.account ?? null,
  ).primary);
  const error = getErrorMessage(state?.error ?? null);
  const status = loading ? '正在读取额度'
    : state?.ordinaryUsageAllowed === false ? '当前使用受限'
      : error?.title ?? (bridgeUnavailable ? '无法连接应用核心'
        : state?.stale ? '数据已过期' : remaining === null ? '额度不可用' : '额度剩余');
  return {
    value: remaining === null ? '--' : `${remaining}%`,
    progress: remaining,
    status: `${status}${state?.stale && remaining !== null ? ' · 陈旧快照' : ''}`,
    marker: loading ? '…' : state?.ordinaryUsageAllowed === false ? '限'
      : error || bridgeUnavailable ? '!' : state?.stale ? '旧' : null,
    // The quota ring follows the displayed amount; freshness/errors remain in status and marker.
    tone: statusTone(state && {
      ...state,
      connection: remaining === null ? state.connection : 'ready',
      stale: false,
      error: null,
    }, remaining),
  };
}

export function nativeOrbVisualState(
  state: CoreState | null,
  loading: boolean,
  bridgeUnavailable: boolean,
  expanded: boolean,
) {
  const view = orbPresentation(state, loading, bridgeUnavailable);
  return {
    percentage: view.progress,
    health: view.tone,
    visible: !expanded,
    collapsed: !expanded,
  } as const;
}

export function FloatingOrb({ state, loading, bridgeUnavailable, onOpen, windowError }: {
  state: CoreState | null; loading: boolean; bridgeUnavailable: boolean;
  onOpen: () => void; windowError: boolean;
}) {
  const view = orbPresentation(state, loading, bridgeUnavailable);
  const progress = view.progress ?? 0;
  const progressBase = annularProgressSegment(100);
  const progressSegment = annularProgressSegment(progress);
  const progressCore = annularProgressCore(progress);
  const opticalId = useId();
  const id = (name: string) => `${opticalId}-${name}`;
  const paint = (name: string) => `url(#${id(name)})`;
  return <button className={`floating-orb tone-${view.tone}`} type="button"
    aria-label={`${view.status} ${view.value}，展开额度面板`}
    aria-expanded={false} title={windowError ? '窗口切换失败，点击重试' : `${view.status}；悬停展开，双击固定`}
    onClick={event => { if (event.detail === 0) onOpen(); }}>
    {/* Painted optical simulation: no desktop sampling or content distortion. */}
    <svg className="orb-optics" viewBox="0 0 100 100" data-progress={progress} aria-hidden="true">
      <defs>
        <clipPath id={id('boundary')}><circle cx="50" cy="50" r="50" /></clipPath>
        <mask id={id('shell')} maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100">
          <circle cx="50" cy="50" r="49.5" fill="white" />
          <circle cx="50" cy="50" r="44.2" fill="black" filter={paint('bloom')} />
        </mask>
        <mask id={id('progress-channel-mask')} maskUnits="userSpaceOnUse" x="0" y="0" width="100" height="100">
          <rect width="100" height="100" fill="black" />
          <path d={progressBase.path} fill="white" fillRule="evenodd" />
        </mask>
        <linearGradient id={id('rim')} x1="12" y1="8" x2="87" y2="94" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#ffffff" stopOpacity=".82" />
          <stop offset=".24" stopColor="#e7f9ff" stopOpacity=".38" />
          <stop offset=".48" stopColor="#a9dcf4" stopOpacity="0" />
          <stop offset="1" stopColor="#edffff" stopOpacity="0" />
        </linearGradient>
        <linearGradient id={id('shell-light')} x1="15" y1="15" x2="86" y2="84" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#f2fbff" stopOpacity=".48" />
          <stop offset=".18" stopColor="#e7f7ff" stopOpacity=".14" />
          <stop offset=".36" stopColor="#e7f7ff" stopOpacity="0" />
          <stop offset=".8" stopColor="#c5e8ed" stopOpacity="0" />
          <stop offset="1" stopColor="#c5e8ed" stopOpacity=".025" />
        </linearGradient>
        <linearGradient id={id('edge-depth')} x1="5" y1="60" x2="31" y2="94" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#193849" stopOpacity="0" />
          <stop offset=".48" stopColor="#193849" stopOpacity=".22" />
          <stop offset="1" stopColor="#193849" stopOpacity="0" />
        </linearGradient>
        <linearGradient id={id('reflection')} x1="22" y1="12" x2="69" y2="35" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="white" stopOpacity=".78" />
          <stop offset=".45" stopColor="white" stopOpacity=".24" />
          <stop offset="1" stopColor="white" stopOpacity="0" />
        </linearGradient>
        <linearGradient id={id('caustic')} x1="89" y1="24" x2="65" y2="95" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#efffff" stopOpacity=".6" />
          <stop offset=".38" stopColor="#50e8f6" stopOpacity=".48" />
          <stop offset=".7" stopColor="#168ec7" stopOpacity=".08" />
          <stop offset="1" stopColor="#a5fbff" stopOpacity=".42" />
        </linearGradient>
        <radialGradient id={id('health-body')} cx="50" cy="50" r="34" gradientUnits="userSpaceOnUse">
          <stop offset=".79" stopColor="var(--accent)" stopOpacity=".52" />
          <stop offset=".805" stopColor="var(--accent-core)" stopOpacity=".84" />
          <stop offset=".845" stopColor="var(--accent-luminous)" stopOpacity=".98" />
          <stop offset=".91" stopColor="var(--accent)" stopOpacity=".86" />
          <stop offset=".965" stopColor="var(--accent-luminous)" stopOpacity="1" />
          <stop offset="1" stopColor="var(--accent-core)" stopOpacity=".84" />
        </radialGradient>
        <radialGradient id={id('health-bloom')} cx="50" cy="50" r="34" gradientUnits="userSpaceOnUse">
          <stop offset=".79" stopColor="var(--accent)" stopOpacity=".44" />
          <stop offset=".89" stopColor="var(--accent-luminous)" stopOpacity=".72" />
          <stop offset="1" stopColor="var(--accent)" stopOpacity=".48" />
        </radialGradient>
        <linearGradient id={id('health-core')} x1="39" y1="17" x2="71" y2="29" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="var(--accent-core)" stopOpacity=".82" />
          <stop offset=".3" stopColor="var(--accent-luminous)" stopOpacity="1" />
          <stop offset=".58" stopColor="var(--accent-core)" stopOpacity=".9" />
          <stop offset=".84" stopColor="var(--accent-luminous)" stopOpacity="1" />
          <stop offset="1" stopColor="var(--accent-core)" stopOpacity=".76" />
        </linearGradient>
        <linearGradient id={id('progress-channel')} x1="20" y1="15" x2="83" y2="88" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#ffffff" stopOpacity=".18" />
          <stop offset=".24" stopColor="#edfaff" stopOpacity=".072" />
          <stop offset=".46" stopColor="#cdeff5" stopOpacity=".034" />
          <stop offset=".63" stopColor="#ffffff" stopOpacity=".012" />
          <stop offset=".82" stopColor="#143b4c" stopOpacity=".066" />
          <stop offset="1" stopColor="#62d9e7" stopOpacity=".11" />
        </linearGradient>
        <radialGradient id={id('channel-surface')} cx="24" cy="18" r="49" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#ffffff" stopOpacity=".52" />
          <stop offset=".24" stopColor="#f2fdff" stopOpacity=".2" />
          <stop offset=".56" stopColor="#ffffff" stopOpacity=".025" />
          <stop offset="1" stopColor="#ffffff" stopOpacity="0" />
        </radialGradient>
        <radialGradient id={id('channel-cyan')} cx="82" cy="76" r="36" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#54e7f3" stopOpacity=".28" />
          <stop offset=".42" stopColor="#8ef4f8" stopOpacity=".1" />
          <stop offset="1" stopColor="#5bcad9" stopOpacity="0" />
        </radialGradient>
        <radialGradient id={id('channel-depth')} cx="16" cy="72" r="31" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor="#092938" stopOpacity=".2" />
          <stop offset=".46" stopColor="#143a4a" stopOpacity=".075" />
          <stop offset="1" stopColor="#173b4b" stopOpacity="0" />
        </radialGradient>
        <filter id={id('bloom')} x="-25%" y="-25%" width="150%" height="150%" colorInterpolationFilters="sRGB">
          <feGaussianBlur stdDeviation="1.5" />
        </filter>
        <filter id={id('soft-bloom')} x="-30%" y="-30%" width="160%" height="160%" colorInterpolationFilters="sRGB">
          <feGaussianBlur stdDeviation="2.25" />
        </filter>
        <filter id={id('energy-bloom')} x="-20%" y="-20%" width="140%" height="140%" colorInterpolationFilters="sRGB">
          <feGaussianBlur stdDeviation="1.55" />
        </filter>
      </defs>
      <g clipPath={paint('boundary')}>
        <circle cx="50" cy="50" r="49.5" fill={paint('shell-light')} mask={paint('shell')} />
        <g fill="none" strokeLinecap="round">
          <path className="orb-edge-interface" d="M 2.1 59.8 A 48.9 48.9 0 0 1 73.7 7.3"
            stroke={paint('rim')} strokeWidth=".85" opacity=".62" />
          <path className="orb-edge-depth" d="M 5 62 C 8 77 17 86 29 91"
            stroke={paint('edge-depth')} strokeWidth="2.6" filter={paint('bloom')} />
          <path d="M 7 49 C 5 29 21 11 40 7 C 49 5 57 7 64 10"
            stroke={paint('reflection')} strokeWidth="4.6" opacity=".42" filter={paint('soft-bloom')} />
          <path d="M 8 35 C 13 20 27 10 43 8" stroke={paint('reflection')} strokeWidth=".85" opacity=".78" />
          <path d="M 10 39 C 13 27 21 18 30 14 C 20 23 16 31 15 40 Z"
            fill={paint('reflection')} opacity=".16" />
          <path d="M 91 39 C 96 50 95 62 88 71 C 85 75 82 78 78 80"
            stroke={paint('caustic')} strokeWidth="3.2" opacity=".72" filter={paint('bloom')} />
          <path d="M 92 48 C 94 59 90 69 82 76" stroke={paint('caustic')} strokeWidth="1.05" opacity=".66" />
          <path d="M 69 93 Q 77 91 82 85" stroke="#80eff9" strokeWidth="1.3" opacity=".44" />
          <path d="M 22 84 Q 26 89 31 90" stroke="#f5ffff" strokeWidth="1.1" opacity=".5" />
        </g>
        <g className="orb-progress-ring" data-progress-sweep={n(progressSegment.sweepDegrees)}
          data-base-sweep="360"
          data-outer-diameter-css={n(ORB_PROGRESS_GEOMETRY.outerDiameterCssPx)}
          data-thickness-css={n(ORB_PROGRESS_GEOMETRY.thicknessCssPx)}>
          <path className="orb-progress-base" d={progressBase.path} fill={paint('progress-channel')}
            opacity=".78" fillRule="evenodd" />
          <g className="orb-progress-base-optics" mask={paint('progress-channel-mask')}>
            <rect className="orb-progress-base-surface" width="100" height="100" fill={paint('channel-surface')} />
            <rect className="orb-progress-base-refraction" width="100" height="100" fill={paint('channel-cyan')} />
            <rect className="orb-progress-base-depth" width="100" height="100" fill={paint('channel-depth')} />
          </g>
          <path className="orb-progress-bloom" d={progressSegment.path} fill={paint('health-bloom')}
            opacity={progress > 0 ? .52 : 0} filter={paint('energy-bloom')} fillRule="evenodd" />
          <path className="orb-progress-segment" d={progressSegment.path} fill={paint('health-body')}
            opacity={progress > 0 ? .88 : 0} fillRule="evenodd" />
          <path className="orb-progress-core" d={progressCore} fill="none" stroke={paint('health-core')}
            strokeWidth="1.35" opacity={progress > 0 ? .98 : 0} strokeLinecap="round" />
        </g>
      </g>
    </svg>
    <span className="orb-value">{view.value}</span>
  </button>;
}
