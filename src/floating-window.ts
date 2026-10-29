import { invoke, isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

export async function resizeFloatingWindow(expanded: boolean) {
  if (isTauri()) {
    const acrylic = await invoke<boolean>('set_floating_expanded', { expanded });
    document.documentElement.dataset.panelGlass = expanded && acrylic ? 'native-acrylic' : 'css-fallback';
  }
}
export async function resizeFloatingMusic(wide: boolean) {
  if (isTauri()) await invoke('set_floating_music_wide', { wide });
}
export async function restoreFloatingWindow() {
  if (isTauri()) await invoke('restore_floating_window');
}
export async function dragFloatingWindow() {
  if (isTauri()) await invoke('drag_floating_window');
}

export async function subscribePosition(onError: () => void) {
  if (!isTauri()) return () => {};
  let timer: ReturnType<typeof setTimeout> | undefined;
  const unlisten = await getCurrentWindow().onMoved(() => {
    clearTimeout(timer);
    timer = setTimeout(() => { void invoke('save_floating_position').catch(onError); }, 200);
  });
  return () => { clearTimeout(timer); unlisten(); };
}
