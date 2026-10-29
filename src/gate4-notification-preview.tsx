import { useCallback, useEffect, useState } from 'react';
import { useGate4AcceptanceNotifications } from './desktop';
import {
  createGate4NotificationState,
  gate4NotificationModeForKey,
  type Gate4NotificationMode,
} from './gate4-notification-fixtures';

export default function Gate4NotificationPreview() {
  const [snapshot, setSnapshot] = useState(() => ({
    mode: 'healthy' as Gate4NotificationMode,
    revision: 0,
    state: createGate4NotificationState('healthy'),
  }));
  const error = useGate4AcceptanceNotifications(snapshot.state);
  const trigger = useCallback((mode: Gate4NotificationMode) => {
    setSnapshot(current => ({
      mode,
      revision: current.revision + 1,
      state: createGate4NotificationState(mode),
    }));
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const mode = gate4NotificationModeForKey(event.key);
      if (!mode || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      event.preventDefault();
      trigger(mode);
    };
    window.addEventListener('keydown', onKeyDown, { capture: true });
    return () => window.removeEventListener('keydown', onKeyDown, { capture: true });
  }, [trigger]);

  return <section aria-label="Gate 4 notification acceptance">
    <h1>Gate 4 Notification Acceptance</h1>
    <p>STATE = {snapshot.mode}</p>
    <p>REVISION = {snapshot.revision}</p>
    <p>F1 healthy · F2 warning · F3 critical · F4 depleted</p>
    <p>F5 stale · F6 error · F7 offline · F8 restricted · F9 no quota</p>
    <p role="status">{error}</p>
  </section>;
}
