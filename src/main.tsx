import React from "react";
import ReactDOM from "react-dom/client";
import { ExpandedQuotaPanel } from "./components/ExpandedQuotaPanel";
import { createGate2PreviewState, isGate2Preview } from "./gate2-preview";
import "./styles.css";
import { useCoreState } from "./use-core-state";
import { FloatingWindow } from "./components/FloatingWindow";
import { Gate3Preview } from "./gate3-preview";
import { SettingsPage, NotificationPage, openSettings } from './desktop';
import { TrayPopup } from './TrayPopup';

const GlassPrototype = import.meta.env.DEV ? React.lazy(() => import('./glass-prototype')) : null;
const Gate4NotificationPreview = import.meta.env.DEV && import.meta.env.VITE_GATE4_ACCEPTANCE === '1'
  ? React.lazy(() => import('./gate4-notification-preview'))
  : null;

function LiveApp() {
  const { state, loading, refreshing, bridgeUnavailable, refresh } = useCoreState();
  const [settingsError, setSettingsError] = React.useState('');

  return (
    <><FloatingWindow state={state} loading={loading} bridgeUnavailable={bridgeUnavailable}>
    {(pinned, togglePin) => (
    <ExpandedQuotaPanel
      pinned={pinned}
      onTogglePin={togglePin}
      state={state}
      loading={loading}
      refreshing={refreshing}
      bridgeUnavailable={bridgeUnavailable}
      onRefresh={refresh}
      onSettings={() => { void openSettings().catch(() => setSettingsError('无法打开设置')); }}
    />
    )}
    </FloatingWindow>{settingsError && <span role="status">{settingsError}</span>}</>
  );
}

function App() {
  const view = new URLSearchParams(location.search).get('view');
  if (view === 'settings') return <SettingsPage />;
  if (view === 'tray-popup') return <TrayPopup />;
  if (view === 'notification') return <main className="app-shell"><NotificationPage /></main>;
  if (Gate4NotificationPreview) return <main className="app-shell"><React.Suspense fallback={null}><Gate4NotificationPreview /></React.Suspense></main>;
  return (
    <main className={isGate2Preview ? "app-shell preview-shell" : "app-shell"}>
      {GlassPrototype && (import.meta.env.VITE_GATE3_PREVIEW === '1' || import.meta.env.VITE_GLASS_PROTOTYPE === '1' || new URLSearchParams(location.search).get('preview') === 'glass') ? <React.Suspense fallback={null}><GlassPrototype /></React.Suspense> : import.meta.env.DEV && new URLSearchParams(location.search).get('preview') === 'gate3' ? <Gate3Preview /> : isGate2Preview ? (
        <ExpandedQuotaPanel
          state={createGate2PreviewState()}
          loading={false}
          refreshing={false}
          bridgeUnavailable={false}
          onRefresh={() => undefined}
        />
      ) : (
        <LiveApp />
      )}
    </main>
  );
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
