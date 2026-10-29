import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

// GHSA-qwgh-2vcv-g2f7 affects block-buffer releases before 0.12.1.
function affectedBlockBuffers(lock) {
  return lock.split(/^\[\[package\]\]\s*$/m).flatMap(section => {
    const name = section.match(/^name\s*=\s*"([^"]+)"\s*$/m)?.[1];
    if (name !== 'block-buffer') return [];
    const version = section.match(/^version\s*=\s*"([^"]+)"\s*$/m)?.[1] ?? '';
    const parts = version.match(/^(\d+)\.(\d+)\.(\d+)(-[^+]+)?(?:\+.*)?$/);
    if (!parts) return [version || '(missing version)'];
    const [major, minor, patch] = parts.slice(1, 4).map(Number);
    const fixed = major > 0 || minor > 12 || (minor === 12 && (patch > 1 || (patch === 1 && !parts[4])));
    return fixed ? [] : [version];
  });
}

test('security check rejects old and prerelease block-buffer versions', () => {
  const lock = version => `[[package]]\nname = "block-buffer"\nversion = "${version}"\n`;
  assert.deepEqual(affectedBlockBuffers(lock('0.10.4')), ['0.10.4']);
  assert.deepEqual(affectedBlockBuffers(lock('0.12.1-rc.1')), ['0.12.1-rc.1']);
  assert.deepEqual(affectedBlockBuffers(lock('0.12.1')), []);
  assert.deepEqual(affectedBlockBuffers(lock('0.13.0')), []);
});

test('locked dependencies do not include the vulnerable block-buffer', () => {
  const lock = readFileSync(new URL('../src-tauri/Cargo.lock', import.meta.url), 'utf8');
  assert.deepEqual(affectedBlockBuffers(lock), []);
});
