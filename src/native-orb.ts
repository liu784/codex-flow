import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { nativeOrbVisualState } from './components/FloatingOrb';

export type NativeOrbVisualState = ReturnType<typeof nativeOrbVisualState>;

export interface NativeOrbStatus {
  featureCompiled: boolean;
  featureEnabled: boolean;
  active: boolean;
  fallbackReason: string | null;
  captureFps: number | null;
  renderFps: number | null;
  dpi: number | null;
}

const LEGACY_STATUS: NativeOrbStatus = {
  featureCompiled: false,
  featureEnabled: false,
  active: false,
  fallbackReason: null,
  captureFps: null,
  renderFps: null,
  dpi: null,
};

export async function updateNativeOrbVisualState(visual: NativeOrbVisualState) {
  if (!isTauri()) return LEGACY_STATUS;
  return invoke<NativeOrbStatus>('update_native_orb_visual_state', { visual });
}

export async function subscribeNativeOrbEvents(
  onOpen: () => void,
  onFallback: (reason: string) => void,
) {
  if (!isTauri()) return () => {};
  const [removeOpen, removeFallback] = await Promise.all([
    listen('native-orb-open', onOpen),
    listen<string>('native-orb-fallback', event => onFallback(event.payload)),
  ]);
  return () => {
    removeOpen();
    removeFallback();
  };
}
