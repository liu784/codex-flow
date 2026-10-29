import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import test from 'node:test';
import ts from 'typescript';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

const require = createRequire(import.meta.url);
function loadTs(path, imports = {}) {
  const source = readFileSync(new URL(path, import.meta.url), 'utf8');
  const { outputText } = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX },
  });
  const module = { exports: {} };
  new Function('require', 'module', 'exports', outputText)(
    id => imports[id] ?? require(id), module, module.exports,
  );
  return module.exports;
}
const core = loadTs('../src/core-state.ts');
const orb = loadTs('../src/components/FloatingOrb.tsx', { '../core-state': core });
const messages = loadTs('../src/island-messages.ts', {
  '@tauri-apps/api/core': { isTauri: () => false },
  '@tauri-apps/api/event': { listen: () => Promise.resolve(() => {}) },
});
const { FloatingIsland } = loadTs('../src/components/FloatingIsland.tsx', {
  './FloatingOrb': orb, './floating-island.css': {},
  '../core-state': core,
  '../island-messages': messages,
  '../island-avatar': { DEFAULT_ISLAND_AVATAR: '/assets/island/default-cat.png', useIslandAvatar: () => ({ avatar: null }) },
});
const props = { loading: false, bridgeUnavailable: false, windowError: false, settings: null, onOpen() {} };
const state = remainingPercent => ({
  windows: [{ limitId: 'codex', planType: 'pro', kind: 'weekly', durationMinutes: 10080, remainingPercent }],
  account: { planType: 'pro' }, connection: 'ready', error: null, stale: false, ordinaryUsageAllowed: true,
});

test('island shows the selected real remaining quota and accessible status', () => {
  const markup = renderToStaticMarkup(createElement(FloatingIsland, { ...props, state: state(21) }));
  assert.match(markup, /21%/);
  assert.match(markup, /assets\/island\/default-cat\.png/);
  assert.doesNotMatch(markup, />CODEX FLOW<|>额度剩余</);
  assert.match(markup, /tone-warning/);
  assert.doesNotMatch(markup, /island-marker/);
  assert.match(markup, /aria-label="额度剩余 21%，额度偏低，展开额度面板"/);
  assert.equal((markup.match(/class="island-digit"/g) ?? []).length, 3);
  assert.match(markup, /animation-delay:0s/);
  assert.match(markup, /animation-delay:0\.25s/);
  assert.match(markup, /animation-delay:0\.5s/);
});

test('zero quota remains zero without a glowing progress fill', () => {
  const markup = renderToStaticMarkup(createElement(FloatingIsland, { ...props, state: state(0) }));
  assert.match(markup, /aria-label="额度剩余 0%，额度严重不足，展开额度面板"/);
  assert.match(markup, /<span class="island-track" aria-hidden="true"><\/span>/);
});

test('yellow and red low-quota states retain their colors without an exclamation mark', () => {
  for (const [remaining, tone] of [[25, 'warning'], [21, 'warning'], [10, 'critical'], [1, 'critical'], [0, 'critical']]) {
    const markup = renderToStaticMarkup(createElement(FloatingIsland, { ...props, state: state(remaining) }));
    assert.match(markup, new RegExp(`tone-${tone}`));
    assert.doesNotMatch(markup, /island-marker|>!<|！/);
    assert.match(markup, new RegExp(`${remaining}%`));
  }
});

test('missing or stale quota stays distinguishable from fresh data', () => {
  const missing = renderToStaticMarkup(createElement(FloatingIsland, { ...props, state: null }));
  assert.match(missing, /额度不可用/);
  assert.match(missing, /--/);
  assert.doesNotMatch(missing, /100%/);
  assert.doesNotMatch(missing, /island-digit/);
  const stale = renderToStaticMarkup(createElement(FloatingIsland, { ...props, state: { ...state(21), stale: true } }));
  assert.match(stale, /21%/);
  assert.match(stale, /陈旧快照/);
  assert.match(stale, /island-marker/);
});

test('disabled quota colors do not hide stale-data status', () => {
  const settings = { quotaAlertsEnabled: false, warningEnabled: false, criticalEnabled: false };
  const muted = renderToStaticMarkup(createElement(FloatingIsland, { ...props, settings, state: state(21) }));
  assert.match(muted, /tone-healthy/);
  assert.doesNotMatch(muted, /island-marker|额度偏低/);
  const stale = renderToStaticMarkup(createElement(FloatingIsland, { ...props, settings, state: { ...state(21), stale: true } }));
  assert.match(stale, /tone-warning/);
  assert.match(stale, /陈旧快照/);
});

test('first connection does not display a stale warning or placeholder numbers', () => {
  const loading = renderToStaticMarkup(createElement(FloatingIsland, { ...props, loading: true, state: null }));
  const connecting = renderToStaticMarkup(createElement(FloatingIsland, { ...props, state: {
    windows: [], account: null, connection: 'connecting', error: null, stale: true,
    ordinaryUsageAllowed: null, lastSuccessAt: null,
  } }));
  const starting = renderToStaticMarkup(createElement(FloatingIsland, { ...props, state: {
    windows: [], account: null, connection: 'disconnected', error: null, stale: true,
    ordinaryUsageAllowed: null, lastSuccessAt: null,
  } }));
  for (const markup of [loading, connecting, starting]) {
    assert.match(markup, /正在连接/);
    assert.doesNotMatch(markup, /数据已过期|island-marker|island-value|island-track|>--</);
  }
});

test('a real stale snapshot and a real connection failure remain visible', () => {
  const stale = renderToStaticMarkup(createElement(FloatingIsland, { ...props, state: {
    ...state(21), stale: true, lastSuccessAt: 123,
  } }));
  assert.match(stale, /21%/);
  assert.match(stale, /陈旧快照/);
  assert.match(stale, /island-marker/);
  const failed = renderToStaticMarkup(createElement(FloatingIsland, { ...props, bridgeUnavailable: true, state: null }));
  assert.match(failed, /无法连接应用核心/);
  assert.match(failed, /class="island-status">无法连接应用核心/);
  assert.doesNotMatch(failed, /island-marker/);
  const coreFailed = renderToStaticMarkup(createElement(FloatingIsland, { ...props, state: {
    windows: [], account: null, connection: 'disconnected', error: 'CODEX_NOT_FOUND', stale: true,
    ordinaryUsageAllowed: null, lastSuccessAt: null,
  } }));
  assert.doesNotMatch(coreFailed, /正在连接/);
  assert.match(coreFailed, /class="island-status">未找到 Codex/);
  assert.doesNotMatch(coreFailed, /island-marker/);
});

test('QQ Music mode drips out right-side secondary album island, stays pure black when not playing, and retracts on close', () => {
  const coverUrl = 'data:image/jpeg;base64,/9j/4AAQSkZJRgABAQAAAQABAAD/';
  const baseLyric = {
    lineStartMs: 1000,
    positionMs: 1200,
    playing: true,
    cover: coverUrl,
    receivedAt: 0,
    words: [{ text: '晴天', startMs: 1000, durationMs: 600 }],
  };
  const openMarkup = renderToStaticMarkup(createElement(FloatingIsland, {
    ...props,
    state: state(80),
    musicWide: true,
    lyric: baseLyric,
  }));
  assert.match(openMarkup, /is-music-wide/);
  assert.match(openMarkup, /class="island-ink-bridge"/);
  assert.match(openMarkup, /class="island-sub-pod"/);
  assert.match(openMarkup, /class="island-sub-cover"/);
  assert.doesNotMatch(openMarkup, /island-ink-goo|island-ink-source|island-sub-disc/);
  assert.ok(openMarkup.includes(`src="${coverUrl}"`));

  const idleMusicMarkup = renderToStaticMarkup(createElement(FloatingIsland, {
    ...props,
    state: state(44),
    musicWide: true,
    musicPlaying: false,
    musicCover: coverUrl,
  }));
  assert.match(idleMusicMarkup, /<span class="island-sub-pod" aria-hidden="true"><\/span>/);
  assert.doesNotMatch(idleMusicMarkup, /island-sub-cover|island-sub-disc/);
  assert.match(idleMusicMarkup, /44%/);

  const closingMarkup = renderToStaticMarkup(createElement(FloatingIsland, {
    ...props,
    state: state(80),
    musicWide: true,
    musicClosing: true,
    musicCover: coverUrl,
    musicPlaying: true,
  }));
  assert.match(closingMarkup, /is-music-wide is-music-closing/);
  assert.match(closingMarkup, /class="island-sub-pod"/);

  const normalMarkup = renderToStaticMarkup(createElement(FloatingIsland, {
    ...props,
    state: state(80),
    musicWide: false,
  }));
  assert.doesNotMatch(normalMarkup, /island-sub-pod/);
  assert.match(normalMarkup, /assets\/island\/default-cat\.png/);
});
