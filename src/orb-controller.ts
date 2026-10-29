// One resize at a time; a newer intent is applied after the in-flight resize.
export function createOrbController(
  resize: (expanded: boolean) => Promise<void>,
  changed: (expanded: boolean) => void,
  failed: () => void,
  schedule = (fn: () => void, ms: number) => setTimeout(fn, ms),
  cancel = (id: ReturnType<typeof setTimeout>) => clearTimeout(id),
) {
  let actual = false;
  let desired = false;
  let busy = false;
  let disposed = false;
  let pinned = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  function clear() {
    if (timer !== undefined) cancel(timer);
    timer = undefined;
  }
  async function flush() {
    if (busy || disposed) return;
    busy = true;
    try {
      while (!disposed && desired !== actual) {
        const next = desired;
        await resize(next);
        actual = next;
        if (!disposed) changed(actual);
      }
    } catch {
      desired = actual;
      if (!disposed) failed();
    } finally {
      busy = false;
    }
  }
  function request(expanded: boolean, delay = 0) {
    clear();
    const apply = () => { desired = expanded; void flush(); };
    if (delay) timer = schedule(apply, delay);
    else apply();
  }
  return {
    request,
    enter() { request(true, 150); },
    leave() { if (!pinned) request(false, 400); },
    togglePin() {
      pinned = !pinned;
      request(true);
      return pinned;
    },
    setPinned(value: boolean) { pinned = value; if (value) clear(); },
    escape() { pinned = false; request(false); },
    cancelPending: clear,
    dispose() { disposed = true; clear(); },
  };
}
