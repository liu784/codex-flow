// DEV prototype only. Physical key codes avoid keyboard-layout-dependent Alt text.
export function glassModeForKey(event: Pick<KeyboardEvent,
  'key' | 'code' | 'altKey' | 'ctrlKey' | 'metaKey' | 'shiftKey' | 'repeat' | 'isComposing'>) {
  if (event.repeat || event.isComposing || event.ctrlKey || event.metaKey || event.shiftKey) return null;
  if (!event.altKey) {
    if (event.code === 'F6' || event.key === 'F6') return 'current';
    if (event.code === 'F7' || event.key === 'F7') return 'standard';
    return null;
  }
  if (['Digit5', 'Numpad5'].includes(event.code) || event.key === '5') return 'current';
  if (['Digit6', 'Numpad6'].includes(event.code) || event.key === '6') return 'standard';
  return null;
}
