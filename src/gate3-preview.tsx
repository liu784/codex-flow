import { useEffect, useState } from 'react';
import { createGate2PreviewState } from './gate2-preview';
import { FloatingWindow } from './components/FloatingWindow';
import { ExpandedQuotaPanel } from './components/ExpandedQuotaPanel';

// Explicit dev-only native acceptance fixture. No raw responses or credentials.
// Alt+1 ready, Alt+2 stale/offline, Alt+3 error without data, Alt+4 restricted.
export function Gate3Preview() {
  const [mode, setMode] = useState('1');
  useEffect(() => {
    const key = (event: KeyboardEvent) => {
      if (event.altKey && ['1', '2', '3', '4'].includes(event.key)) {
        event.preventDefault(); setMode(event.key);
      }
    };
    window.addEventListener('keydown', key);
    return () => window.removeEventListener('keydown', key);
  }, []);
  const state = createGate2PreviewState();
  if (mode === '2') { state.stale = true; state.error = 'OFFLINE'; state.connection = 'offline'; }
  if (mode === '3') { state.windows = []; state.error = 'CODEX_NOT_FOUND'; state.account = null; }
  if (mode === '4') state.ordinaryUsageAllowed = false;
  return <FloatingWindow state={state} loading={false} bridgeUnavailable={false}>
    {(pinned, togglePin) => <ExpandedQuotaPanel state={state} loading={false} refreshing={false}
      bridgeUnavailable={false} onRefresh={() => {}} pinned={pinned} onTogglePin={togglePin} />}
  </FloatingWindow>;
}
