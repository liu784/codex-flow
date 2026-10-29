import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

test('ordinary desktop builds include the native marquee and edge-dock backend', () => {
  const cargo = readFileSync(new URL('../src-tauri/Cargo.toml', import.meta.url), 'utf8');
  const features = cargo.match(/^\[features\]\s*\n([\s\S]*?)(?=^\[|$(?![\s\S]))/m)?.[1];
  assert.ok(features, 'Cargo features must be declared');
  assert.match(features, /^default\s*=\s*\[[^\]]*"native-orb-refraction"[^\]]*\]/m,
    'Native Orb must not depend on a manually supplied release flag');
});
