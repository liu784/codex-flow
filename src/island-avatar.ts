import { useEffect, useSyncExternalStore } from 'react';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export const DEFAULT_ISLAND_AVATAR = '/assets/island/default-cat.png';
const MAX_FILE_BYTES = 5 * 1024 * 1024;
const MAX_PIXELS = 24_000_000;
type Snapshot = { avatar: string | null; ready: boolean; saving: boolean; error: string };
let snapshot: Snapshot = { avatar: null, ready: false, saving: false, error: '' };
const subscribers = new Set<() => void>();
let started: Promise<void> | undefined;
let revision = 0;
class AvatarInputError extends Error {}
function publish(next: Partial<Snapshot>) {
  snapshot = { ...snapshot, ...next };
  subscribers.forEach(fn => fn());
}
async function reloadAvatar() {
  const before = revision;
  try {
    const avatar = await invoke<string | null>('get_island_avatar');
    if (before === revision) publish({ avatar, ready: true, error: '' });
  } catch {
    if (before === revision) publish({ ready: true, error: '无法读取头像，请重试' });
  }
}
async function start() {
  if (!isTauri()) { publish({ ready: true }); return; }
  await listen('island-avatar-changed', () => { revision++; void reloadAvatar(); });
  await reloadAvatar();
  window.addEventListener('focus', () => { void reloadAvatar(); });
}
export function useIslandAvatar() {
  useEffect(() => { started ??= start().catch(() => publish({ ready: true, error: '无法连接头像服务' })); }, []);
  return useSyncExternalStore(fn => { subscribers.add(fn); return () => { subscribers.delete(fn); }; }, () => snapshot);
}

function supportedImage(bytes: Uint8Array) {
  const png = [137, 80, 78, 71, 13, 10, 26, 10].every((value, index) => bytes[index] === value);
  const jpeg = bytes[0] === 255 && bytes[1] === 216 && bytes[2] === 255;
  const webp = [82, 73, 70, 70].every((value, index) => bytes[index] === value)
    && [87, 69, 66, 80].every((value, index) => bytes[index + 8] === value);
  return png || jpeg || webp;
}
async function prepareAvatar(file: File) {
  if (file.size > MAX_FILE_BYTES) throw new AvatarInputError('图片不能超过 5 MB');
  const signature = new Uint8Array(await file.slice(0, 12).arrayBuffer());
  if (!supportedImage(signature)) throw new AvatarInputError('请选择 PNG、JPG 或 WebP 图片');
  const url = URL.createObjectURL(file);
  try {
    const image = new Image();
    const loaded = new Promise<void>((resolve, reject) => {
      image.onload = () => resolve();
      image.onerror = () => reject(new AvatarInputError('图片无法读取，请换一张'));
    });
    image.src = url;
    await loaded;
    const { naturalWidth: width, naturalHeight: height } = image;
    if (!width || !height || width > 8192 || height > 8192 || width * height > MAX_PIXELS) {
      throw new AvatarInputError('图片尺寸过大，请换一张');
    }
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = 256;
    const context = canvas.getContext('2d');
    if (!context) throw new AvatarInputError('图片无法处理，请换一张');
    const side = Math.min(width, height);
    context.drawImage(image, (width - side) / 2, (height - side) / 2, side, side, 0, 0, 256, 256);
    return canvas.toDataURL('image/png');
  } finally {
    URL.revokeObjectURL(url);
  }
}
async function saveAvatar(avatar: string | null) {
  const before = revision;
  await invoke('set_island_avatar', { avatar });
  if (before === revision) { revision++; publish({ avatar }); }
}
export async function chooseIslandAvatar(file: File) {
  publish({ saving: true, error: '' });
  try { await saveAvatar(await prepareAvatar(file)); }
  catch (error) {
    publish({ error: error instanceof AvatarInputError ? error.message : '头像未能保存，请重试' });
    throw error;
  } finally { publish({ saving: false }); }
}
export async function resetIslandAvatar() {
  publish({ saving: true, error: '' });
  try { await saveAvatar(null); }
  catch (error) { publish({ error: '头像未能保存，请重试' }); throw error; }
  finally { publish({ saving: false }); }
}
