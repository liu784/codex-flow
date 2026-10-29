import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import test from 'node:test';
import ts from 'typescript';
import { createElement } from 'react';
import { renderToStaticMarkup } from 'react-dom/server';

// Compile the actual implementation in memory; no generated files or new dependencies.
const require = createRequire(import.meta.url);
function loadTs(path, imports = {}) {
  const source = readFileSync(new URL(path, import.meta.url), 'utf8').replaceAll('import.meta.env.DEV', 'false');
  const { outputText } = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, jsx: ts.JsxEmit.ReactJSX },
  });
  const module = { exports: {} };
  new Function('require', 'module', 'exports', outputText)(
    (id) => imports[id] ?? (id === '../app-settings' ? { useAppSettings: () => ({ settings: null }) } : require(id)), module, module.exports,
  );
  return module.exports;
}
const core = loadTs('../src/core-state.ts');
const cat = loadTs('../src/cat-controller.ts');
const { CharacterPortrait } = loadTs('../src/components/CharacterPortrait.tsx', {
  '../cat-controller': cat,
  '../cat-head-renderer': { CatHeadRenderer: class {} },
});
test('CURRENT material keeps clear content and clipped surfaces without displacement', () => {
  // Source guard only; native screenshot acceptance is still required.
  const css = readFileSync(new URL('../src/styles.css', import.meta.url), 'utf8');
  const material = css.match(/\.glass\s*\{([^}]+)\}/)[1];
  assert.match(material, /backdrop-filter: blur/);
  assert.doesNotMatch(material, /(?:^|[;\n])\s*(?:filter|transform|opacity)\s*:|url\(|repeating-/);
  assert.doesNotMatch(material, /\n\s+0 16px 42px/);
  assert.match(css, /\.orb-optics\s*\{[\s\S]*?pointer-events: none/);
  assert.doesNotMatch(css, /\.floating-orb\.glass\s*\{/);
  const panel = css.match(/\.quota-panel\s*\{([^}]+)\}/)[1];
  assert.match(panel, /overflow:\s*hidden/);
  assert.match(panel, /border-radius:\s*var\(--panel-radius\)/);
  assert.match(css, /--glass-blur: 8px/);
  assert.match(css, /--glass-tint: rgba\(63, 185, 211, 0\.045\)/);
  assert.match(css, /--glass-bg-a: rgba\(245, 252, 255, 0\.07\)/);
  assert.match(css, /--glass-bg-b: rgba\(214, 243, 250, 0\.025\)/);
  assert.match(css, /--glass-saturate: 1\.08/);
  assert.match(css, /\.floating-orb\s*\{[\s\S]*?width: 88px;[\s\S]*?height: 88px;/);
  const content = css.match(/\.orb-value\s*\{([^}]+)\}/)[1];
  assert.doesNotMatch(content, /(?:^|[;\n])\s*(?:filter|opacity)\s*:/);
  assert.doesNotMatch(content, /(?:^|[;\n])\s*(?:background|background-color|backdrop-filter)\s*:/);
  assert.match(content, /-webkit-text-stroke: 0\.5px rgba\(15, 35, 49, 0\.48\)/);
  assert.match(content, /paint-order: stroke fill/);
  assert.match(content, /text-shadow: 0 0\.5px 0\.7px rgba\(15, 35, 49, 0\.72\),\s*0 0 1\.2px rgba\(15, 35, 49, 0\.48\)/);
});
const { glassModeForKey } = loadTs('../src/gate3-glass-shortcut.ts');
test('Gate 3 material shortcuts support key/code and non-Alt fallback', () => {
  const base = { key: '', code: '', altKey: false, ctrlKey: false, metaKey: false, shiftKey: false, repeat: false, isComposing: false };
  for (const [key, code, altKey, expected] of [
    ['5', '', true, 'current'], ['6', '', true, 'standard'],
    ['§', 'Digit5', true, 'current'], ['^', 'Digit6', true, 'standard'],
    ['', 'Numpad5', true, 'current'], ['', 'Numpad6', true, 'standard'],
    ['F6', 'F6', false, 'current'], ['F7', 'F7', false, 'standard'],
    ['1', 'Digit1', true, null], ['5', 'Digit5', false, null],
  ]) assert.equal(glassModeForKey({ ...base, key, code, altKey }), expected);
  for (const modifier of ['ctrlKey', 'metaKey', 'shiftKey', 'repeat', 'isComposing']) {
    assert.equal(glassModeForKey({ ...base, key: '6', altKey: true, [modifier]: true }), null);
  }
});
const { ExpandedQuotaPanel } = loadTs('../src/components/ExpandedQuotaPanel.tsx', {
  '../cat-controller': cat,
  '../core-state': core,
  './CharacterPortrait': { CharacterPortrait },
  './RefreshAction': loadTs('../src/components/RefreshAction.tsx'),
});
const {
  FloatingOrb,
  ORB_PROGRESS_GEOMETRY,
  annularProgressCore,
  annularProgressSegment,
  nativeOrbVisualState,
  orbPresentation,
} = loadTs('../src/components/FloatingOrb.tsx', { '../core-state': core });

test('native orb bridge serializes only final presentation state', () => {
  const state = {
    windows: [windowFor('pro', 'weekly', 10080)],
    account: { planType: 'pro' },
    connection: 'ready', error: null, stale: false,
  };
  assert.deepEqual(nativeOrbVisualState(state, false, false, false), {
    percentage: 72,
    health: 'healthy',
    visible: true,
    collapsed: true,
  });
  assert.deepEqual(nativeOrbVisualState(state, false, false, true), {
    percentage: 72,
    health: 'healthy',
    visible: false,
    collapsed: false,
  });
  assert.deepEqual(nativeOrbVisualState({ windows: [] }, false, false, false), {
    percentage: null,
    health: 'neutral',
    visible: true,
    collapsed: true,
  });
});
test('native orb keeps a real low-quota value and critical health at startup', () => {
  const state = {
    windows: [{
      ...windowFor('pro', 'weekly', 10080),
      usedPercent: 95,
      remainingPercent: 5,
    }],
    account: { planType: 'pro' },
    connection: 'ready', error: null, stale: false,
  };
  assert.deepEqual(nativeOrbVisualState(state, false, false, false), {
    percentage: 5,
    health: 'critical',
    visible: true,
    collapsed: true,
  });
});
test('orb SVG confines optics, isolates sharp content and scopes gradient IDs', () => {
  const props = { state: null, loading: false, bridgeUnavailable: false, onOpen() {}, windowError: false };
  const markup = renderToStaticMarkup(createElement('div', null,
    createElement(FloatingOrb, props), createElement(FloatingOrb, props)));
  assert.match(markup, /<clipPath[^>]*><circle cx="50" cy="50" r="50"/);
  assert.match(markup, /maskUnits="userSpaceOnUse"/);
  assert.match(markup, /<\/svg><span class="orb-value">--<\/span>/);
  assert.match(markup, /<g class="orb-progress-ring"[^>]*data-progress-sweep="0"/);
  assert.match(markup, /data-base-sweep="360"/);
  assert.match(markup, /<mask[^>]*id="[^"]*-progress-channel-mask"[\s\S]*?<path[^>]*fill="white"[^>]*fill-rule="evenodd"/);
  assert.match(markup, /<path class="orb-progress-base"[^>]*fill="url\(#[^"]*-progress-channel\)"[^>]*fill-rule="evenodd"/);
  assert.match(markup, /class="orb-progress-base-optics"[^>]*mask="url\(#[^"]*-progress-channel-mask\)"/);
  assert.match(markup, /class="orb-progress-base-surface"/);
  assert.match(markup, /class="orb-progress-base-refraction"/);
  assert.match(markup, /class="orb-progress-base-depth"/);
  assert.match(markup, /<path class="orb-progress-segment"/);
  assert.doesNotMatch(markup, /orb-ring-track|stroke-dasharray|stroke-dashoffset/);
  assert.doesNotMatch(markup, /feDisplacementMap|feTurbulence|悬浮球（折叠状态）/);
  const ids = [...markup.matchAll(/\sid="([^"]+)"/g)].map(match => match[1]);
  assert.equal(new Set(ids).size, ids.length);
});
test('orb edge depth stays local without a filled core or complete optical outline', () => {
  const markup = renderToStaticMarkup(createElement(FloatingOrb, {
    state: null, loading: false, bridgeUnavailable: false, onOpen() {}, windowError: false,
  }));
  const shell = markup.split('</defs>')[1].split('<g class="orb-progress-ring"')[0];
  assert.equal((shell.match(/<circle\b/g) ?? []).length, 1);
  assert.match(shell, /<circle[^>]*mask="url\(/);
  assert.doesNotMatch(shell, /<circle[^>]*\sstroke=|radialGradient|orb-optical-core/);
  assert.match(shell, /<path class="orb-edge-interface"[^>]*stroke-width="\.85"/);
  assert.match(shell, /<path class="orb-edge-depth"/);
  assert.match(markup, /<circle cx="50" cy="50" r="44\.2" fill="black"/);
  assert.match(markup, /class="orb-progress-base"[^>]*opacity="\.78"/);
  assert.match(markup, /class="orb-progress-base-optics"/);
  assert.match(markup, /class="orb-progress-segment"[^>]*fill="url\(#[^"]*-health-body\)"[^>]*opacity="0"/);
  assert.match(markup, /class="orb-progress-core"/);
  assert.doesNotMatch(markup, /orb-progress-base-highlight|progress-base-glass|health-energy|orb-progress-glint/);
  assert.doesNotMatch(markup, /orb-ring-track|orb-ring-value|stroke-dasharray|stroke-dashoffset/);
});
test('orb progress uses true annular geometry with an exact dynamic sweep', () => {
  assert.ok(Math.abs(ORB_PROGRESS_GEOMETRY.outerDiameterCssPx - 59.84) < 0.001);
  assert.ok(Math.abs(ORB_PROGRESS_GEOMETRY.innerRadiusCssPx - 23.936) < 0.001);
  assert.ok(Math.abs(ORB_PROGRESS_GEOMETRY.thicknessCssPx - 5.984) < 0.001);

  const ninePercent = annularProgressSegment(9);
  assert.equal(ninePercent.progress, 9);
  assert.ok(Math.abs(ninePercent.sweepDegrees - 32.4) < 0.001);
  assert.match(ninePercent.path, /A 34 34/);
  assert.match(ninePercent.path, /A 27\.2 27\.2/);
  assert.match(ninePercent.path, /A 3\.4 3\.4/);
  assert.match(annularProgressCore(9), /A 30\.6 30\.6/);

  assert.equal(annularProgressSegment(0).path, '');
  assert.ok(Math.abs(annularProgressSegment(72).sweepDegrees - 259.2) < 0.001);
  const fullBase = annularProgressSegment(150);
  assert.equal(fullBase.sweepDegrees, 360);
  assert.match(fullBase.path, /A 34 34/);
  assert.match(fullBase.path, /A 27\.2 27\.2/);
});
function windowFor(planType, kind, durationMinutes) {
  return { planType, kind, durationMinutes, limitId: 'codex', sourceSlot: 'primary',
    usedPercent: 28, remainingPercent: 72, resetsAt: null };
}
function render(windows, planType, overrides = {}) {
  return renderToStaticMarkup(createElement(ExpandedQuotaPanel, {
    state: { windows, account: { planType }, connection: 'ready', error: null,
      ordinaryUsageAllowed: true, stale: false, resetCreditsAvailable: null, ...overrides },
    loading: false, refreshing: false, bridgeUnavailable: false, onRefresh() {},
  }));
}

test('no quota and every error render unavailable metrics without the retired progress UI', () => {
  for (const error of [null, 'CODEX_NOT_FOUND', 'NOT_AUTHENTICATED', 'UNSUPPORTED_CODEX', 'OFFLINE']) {
    const html = render([], null, { error });
    assert.doesNotMatch(html, /class="progress-fill"|aria-valuenow=/);
    assert.match(html, /<strong>--<\/strong><span>剩余额度<\/span>/);
  }
});
test('zero, missing and stale percentages keep their exact metric presentation', () => {
  const quota = windowFor('plus', 'five_hour', 300);
  assert.match(render([{ ...quota, remainingPercent: null }], 'plus'), /<strong>--<\/strong><span>剩余额度<\/span>/);
  assert.match(render([{ ...quota, remainingPercent: 0 }], 'plus'), /<strong>0%<\/strong><span>剩余额度<\/span>/);
  const stale = render([quota], 'plus', { stale: true, error: 'OFFLINE' });
  assert.match(stale, /<strong>72%<\/strong><span>剩余额度<\/span>/);
  assert.doesNotMatch(stale, /class="progress-fill"|--quota-progress/);
});
test('island window keeps a transparent boundary without the native shadow', () => {
  const config = JSON.parse(readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'));
  assert.equal(config.app.windows[0].transparent, true);
  assert.equal(config.app.windows[0].backgroundColor, '#00000000');
  assert.equal(config.app.windows[0].shadow, false);
  assert.equal(config.app.windows[0].width, 280);
  assert.equal(config.app.windows[0].height, 62);
});
for (const plan of ['prolite', 'pro', 'plus', 'unknown', null]) {
  test(`${plan}: actual 300 + 10080 survive selection and panel rendering`, () => {
    const five = { ...windowFor(plan, 'five_hour', 300), remainingPercent: 65 };
    const weekly = windowFor(plan, 'weekly', 10080);
    const windows = Object.freeze([Object.freeze(weekly), Object.freeze(five)]);
    const selected = core.selectQuotaWindows(windows);
    assert.equal(selected.primary, five);
    assert.equal(selected.secondaryWeekly, weekly);
    const html = render(windows, plan);
    if (plan === 'pro' || plan === 'prolite') {
      assert.match(html, /<strong>72%<\/strong><span>剩余额度<\/span>/);
      assert.doesNotMatch(html, /65%/);
    } else {
      assert.match(html, /<strong>65%<\/strong><span>剩余额度<\/span>/);
      assert.doesNotMatch(html, /<strong>72%<\/strong>/);
    }
    assert.doesNotMatch(html, /本周剩余|5 小时剩余|class="progress-rail"/);
  });
  test(`${plan}: weekly-only never fabricates five_hour`, () => {
    const weekly = windowFor(plan, 'weekly', 10080);
    assert.deepEqual(core.selectQuotaWindows([weekly]), {
      primary: weekly, primaryKind: 'weekly', secondaryWeekly: null,
    });
    assert.doesNotMatch(render([weekly], plan), /5 小时/);
  });
}
test('core selection keeps real windows while Pro presentation remains weekly-only', () => {
  const windows = [windowFor('pro', 'weekly', 10080), { ...windowFor('plus', 'five_hour', 300), remainingPercent: 65 }];
  assert.equal(core.selectQuotaWindows(windows).primaryKind, 'five_hour');
  assert.match(render(windows, 'prolite'), /<strong>72%<\/strong><span>剩余额度<\/span>/);
  assert.match(render(windows, 'pro'), /<strong>72%<\/strong><span>剩余额度<\/span>/);
  assert.doesNotMatch(render(windows, 'pro'), /65%|5 小时剩余|本周剩余/);
});
test('unknown plan with only 300 exposes five_hour', () => {
  assert.equal(core.selectQuotaWindows([windowFor('future', 'five_hour', 300)]).primaryKind, 'five_hour');
});
test('non-Pro custom/unknown fallback and none never imply five_hour', () => {
  for (const plan of ['plus', 'future', null]) {
    for (const [kind, duration] of [['custom', 60], ['unknown', null], ['unknown', 0], ['unknown', -1]]) {
      const window = windowFor(plan, kind, duration);
      assert.equal(core.selectQuotaWindows([window]).primary, window);
      assert.match(render([window], plan), /<strong>72%<\/strong><span>剩余额度<\/span>/);
      assert.doesNotMatch(render([window], plan), /5 小时|本周剩余/);
    }
    assert.equal(core.selectQuotaWindows([]).primaryKind, null);
    assert.match(render([], plan), /<strong>--<\/strong><span>剩余额度<\/span>/);
  }
});
test('priority and limitId pairing do not remove or mutate input windows', () => {
  const custom = windowFor('plus', 'custom', 60);
  const otherWeek = { ...windowFor('plus', 'weekly', 10080), limitId: 'other' };
  const weekly = windowFor('pro', 'weekly', 10080);
  const five = windowFor(null, 'five_hour', 300);
  const windows = Object.freeze([custom, otherWeek, weekly, five].map(Object.freeze));
  assert.equal(core.selectQuotaWindows(windows).secondaryWeekly, weekly);
  assert.equal(core.selectQuotaWindows(windows.slice(0, 3)).primary, otherWeek);
  assert.equal(core.selectQuotaWindows([custom]).primary, custom);
  assert.equal(windows.length, 4);
});

test('main Codex bucket wins over model-specific five-hour and weekly windows', () => {
  const codexWeekly = {
    ...windowFor('pro', 'weekly', 10080),
    usedPercent: 80,
    remainingPercent: 20,
    resetsAt: 1_900_000_000,
  };
  const modelFive = {
    ...windowFor('pro', 'five_hour', 300),
    limitId: 'codex_bengalfox',
    usedPercent: 0,
    remainingPercent: 100,
    resetsAt: 1_800_000_000,
  };
  const modelWeekly = {
    ...windowFor('pro', 'weekly', 10080),
    limitId: 'codex_bengalfox',
    usedPercent: 0,
    remainingPercent: 100,
    resetsAt: 1_800_100_000,
  };
  const windows = [modelFive, modelWeekly, codexWeekly];

  const selected = core.selectMainQuotaWindows(windows);
  assert.equal(selected.primary, codexWeekly);
  assert.equal(selected.primary.limitId, 'codex');
  assert.equal(selected.primary.usedPercent, 80);
  assert.equal(selected.primary.remainingPercent, 20);
  assert.equal(selected.primary.resetsAt, codexWeekly.resetsAt);

  const state = { windows, account: { planType: 'pro' }, connection: 'ready', error: null, stale: false };
  assert.deepEqual(
    { value: orbPresentation(state, false).value, progress: orbPresentation(state, false).progress },
    { value: '20%', progress: 20 },
  );
  const orb = renderToStaticMarkup(createElement(FloatingOrb, {
    state, loading: false, onOpen() {}, bridgeUnavailable: false,
  }));
  assert.match(orb, /data-progress="20"/);
  assert.match(orb, /data-progress-sweep="72"/);
  const html = render(windows, 'pro');
  assert.match(html, />20%<\/strong>/);
  assert.doesNotMatch(html, />100%<\/strong>/);
});

test('model-specific-only buckets leave the main weekly quota unavailable', () => {
  const modelFive = { ...windowFor('pro', 'five_hour', 300), limitId: 'codex_bengalfox', remainingPercent: 100 };
  const modelWeekly = { ...windowFor('pro', 'weekly', 10080), limitId: 'codex_bengalfox', remainingPercent: 100 };
  const windows = [modelFive, modelWeekly];

  assert.deepEqual(core.selectMainQuotaWindows(windows), {
    primary: null,
    primaryKind: null,
    secondaryWeekly: null,
  });
  const state = { windows, account: { planType: 'pro' }, connection: 'ready', error: null, stale: false };
  assert.deepEqual(
    { value: orbPresentation(state, false).value, progress: orbPresentation(state, false).progress },
    { value: '--', progress: null },
  );
  const orb = renderToStaticMarkup(createElement(FloatingOrb, {
    state, loading: false, onOpen() {}, bridgeUnavailable: false,
  }));
  assert.match(orb, /data-progress="0"/);
  assert.match(orb, /data-progress-sweep="0"/);
  const html = render(windows, 'pro');
  assert.match(html, />--<\/strong>/);
  assert.doesNotMatch(html, />100%<\/strong>/);
});

for (const [kind, duration] of [['five_hour', 300], ['weekly', 10080], ['custom', 60], ['unknown', null]]) {
  test(`non-Pro orb preferred ${kind} keeps existing real-window selection`, () => {
    for (const planType of ['plus', 'future', null]) {
      const state = { windows: [windowFor(planType, kind, duration)], account: { planType }, connection: 'ready', error: null, stale: false };
      assert.equal(orbPresentation(state, false).value, '72%');
      const markup = renderToStaticMarkup(createElement(FloatingOrb, { state, loading: false, onOpen() {}, bridgeUnavailable: false }));
      assert.match(markup, /72%/);
    }
  });
}
test('Pro weekly fixture drives orb and the remaining metric while five_hour stays hidden', () => {
  const five = { ...windowFor('pro', 'five_hour', 300), remainingPercent: 80 };
  const weekly = { ...windowFor('pro', 'weekly', 10080), usedPercent: 71, remainingPercent: 29 };
  const state = { windows: [five, weekly], account: { planType: 'pro' }, connection: 'ready', error: null, stale: false };
  assert.equal(orbPresentation(state, false).value, '29%');
  const html = render([five, weekly], 'pro');
  assert.match(html, /剩余额度/);
  assert.match(html, />29%<\/strong>/);
  assert.doesNotMatch(html, /5 小时剩余|80%/);
});
test('Pro missing weekly shows dashes without five_hour, custom, unknown, or 100% fallback', () => {
  const cases = [
    [],
    [windowFor('pro', 'five_hour', 300)],
    [windowFor('pro', 'custom', 60)],
    [windowFor('pro', 'unknown', null)],
  ];
  for (const windows of cases) {
    const state = { windows, account: { planType: 'pro' }, connection: 'ready', error: null, stale: false };
    assert.equal(orbPresentation(state, false).value, '--');
    const html = render(windows, 'pro');
    assert.match(html, /剩余额度/);
    assert.match(html, />--<\/strong>/);
    assert.doesNotMatch(html, /5 小时剩余|自定义周期剩余|100%|class="progress-fill"/);
  }
});
test('expanded character panel has one package title, three metrics and no retired quota layout', () => {
  const weekly = { ...windowFor('pro', 'weekly', 10080), resetsAt: Math.floor(Date.now() / 1000) + 86_400 };
  const html = render([weekly], 'pro', { resetCreditsAvailable: 2 });
  assert.match(html, /class="package-title">CODEX · PRO<\/h1>/);
  assert.equal((html.match(/class="panel-metric metric-/g) ?? []).length, 3);
  assert.match(html, /<strong>72%<\/strong><span>剩余额度<\/span>/);
  assert.match(html, /<strong>2次<\/strong><span>可用重置<\/span>/);
  assert.match(html, /<span>下次重置<\/span>/);
  assert.equal((html.match(/class="cat-body-layer"/g) ?? []).length, 1);
  assert.equal((html.match(/class="cat-head-layer"/g) ?? []).length, 1);
  assert.doesNotMatch(html, /character-eye|character-pupil/);
  assert.doesNotMatch(html, /近期检测到消耗|本周剩余|progress-rail|panel-footer|secondary-quota/);
});
test('Pro unavailable rateLimits shows dashes and never manufactures 100%', () => {
  const state = { windows: [], account: { planType: 'pro' }, connection: 'offline', error: 'OFFLINE', stale: false };
  assert.equal(orbPresentation(state, false).value, '--');
  const html = render([], 'pro', { connection: 'offline', error: 'OFFLINE' });
  assert.match(html, />--<\/strong>/);
  assert.doesNotMatch(html, /100%|class="progress-fill"|aria-valuenow=/);
});
test('Pro stale weekly is used only when its weekly snapshot remains valid', () => {
  const weekly = { ...windowFor('pro', 'weekly', 10080), remainingPercent: 29 };
  const valid = { windows: [weekly], account: { planType: 'pro' }, stale: true, error: 'OFFLINE' };
  const invalid = { ...valid, windows: [{ ...weekly, remainingPercent: null }] };
  assert.equal(orbPresentation(valid, false).value, '29%');
  assert.equal(orbPresentation(invalid, false).value, '--');
});
test('orb no quota/loading never manufactures 100%', () => {
  assert.equal(orbPresentation({ windows: [] }, false).value, '--');
  assert.equal(orbPresentation({ windows: [], connection: 'connecting' }, false).tone, 'neutral');
  assert.equal(orbPresentation(null, true).value, '--');
  assert.equal(orbPresentation(null, true).marker, '…');
});
test('orb stale and restricted retain actual quota with explicit status', () => {
  const state = { windows: [windowFor('pro', 'weekly', 10080)], stale: true, ordinaryUsageAllowed: false };
  const view = orbPresentation(state, false);
  assert.equal(view.value, '72%'); assert.match(view.status, /使用受限.*陈旧/); assert.equal(view.tone, 'critical');
  assert.match(orbPresentation({ ...state, ordinaryUsageAllowed: true }, false).status, /过期/);
});
test('orb every error with/without stale never replaces quota with 100%', () => {
  for (const error of ['CODEX_NOT_FOUND', 'NOT_AUTHENTICATED', 'UNSUPPORTED_CODEX', 'OFFLINE']) {
    const state = { windows: [windowFor('pro', 'weekly', 10080)], error, stale: true };
    assert.equal(orbPresentation(state, false).value, '72%');
    assert.match(orbPresentation(state, false).status, /陈旧/);
    assert.equal(orbPresentation({ ...state, stale: false, windows: [] }, false).value, '--');
    assert.equal(orbPresentation(state, false).marker, '!');
  }
});

test('orb quota color survives stale and failed refreshes without hiding status', () => {
  for (const [remaining, health] of [[85, 'healthy'], [26, 'healthy'], [25, 'warning'], [10, 'critical'], [0, 'critical']]) {
    const state = {
      windows: [{ ...windowFor('pro', 'weekly', 10080), usedPercent: 100 - remaining, remainingPercent: remaining }],
      account: { planType: 'pro' }, connection: 'ready', error: null, stale: false,
    };
    for (const transition of [{}, { connection: 'connecting' }, { stale: true }, { stale: true, error: 'OFFLINE', connection: 'offline' }, {}]) {
      const current = { ...state, ...transition };
      const view = orbPresentation(current, false);
      assert.equal(view.tone, health);
      assert.equal(nativeOrbVisualState(current, false, false, false).health, health);
      assert.equal(view.value, `${remaining}%`);
      if (current.stale) assert.match(view.status, /陈旧快照/);
      if (current.error) assert.equal(view.marker, '!');
    }
    assert.equal(orbPresentation({ ...state, ordinaryUsageAllowed: false }, false).tone, 'critical');
  }
});
