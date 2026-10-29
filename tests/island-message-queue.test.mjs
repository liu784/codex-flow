import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import ts from 'typescript';

function setup() {
  const effects = [], handlers = new Map(), timers = new Map();
  let displayed = null, nextTimer = 0;
  const react = {
    useState: initial => [initial, value => { displayed = value; }],
    useRef: current => ({ current }),
    useEffect: fn => effects.push(fn),
  };
  const module = { exports: {} };
  const code = ts.transpileModule(readFileSync('src/island-messages.ts', 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  new Function('require', 'module', 'exports', 'setTimeout', 'clearTimeout', code)(
    name => name === 'react' ? react : name.endsWith('/core') ? { isTauri: () => true } : {
      listen: (name, fn) => { handlers.set(name, fn); return Promise.resolve(() => handlers.delete(name)); },
    }, module, module.exports,
    fn => { timers.set(++nextTimer, fn); return nextTimer; }, id => timers.delete(id),
  );
  module.exports.useIslandMessages();
  const cleanups = effects.map(fn => fn());
  return {
    emit: (source, text) => handlers.get('island-message')({ payload: { source, text } }),
    current: () => displayed,
    advance: () => { const first = timers.entries().next().value; if (first) { timers.delete(first[0]); first[1](); } },
    clear: () => handlers.get('island-message-clear')({}),
    cleanup: () => cleanups.forEach(fn => fn?.()),
    duration: module.exports.messageDisplayDuration,
  };
}

test('simultaneous Grok and Codex replies are shown in order rather than overwritten', () => {
  const s = setup();
  s.emit('Grok Bot', '助手甲：第一条');
  s.emit('Grok Bot', '助手乙：第二条');
  s.emit('Codex', '任务已完成');
  assert.equal(s.current().text, '助手甲：第一条');
  s.advance(); assert.equal(s.current().text, '助手乙：第二条');
  s.advance(); assert.equal(s.current().source, 'Codex');
  s.advance(); assert.equal(s.current(), null);
  s.cleanup();
});

test('long Codex replies keep their ending and have enough time to scroll', () => {
  const s = setup();
  const message = `开头 ${'猫🐱'.repeat(1000)} 结尾`;
  s.emit('Codex', message);
  assert.equal(s.current().text, message);
  assert.ok(s.duration(message) > s.duration('猫'.repeat(500)));
  s.cleanup();
});

test('duplicate queued events do not repeat and disabling clears all pending messages', () => {
  const s = setup();
  s.emit('Grok Bot', '助手甲：第一条');
  s.emit('Grok Bot', '助手乙：第二条');
  s.emit('Grok Bot', '助手乙：第二条');
  s.advance(); assert.equal(s.current().text, '助手乙：第二条');
  s.advance(); assert.equal(s.current(), null);
  s.emit('Grok Bot', '小助手：完成');
  s.emit('Codex', '另一条回复');
  s.clear(); s.advance();
  assert.equal(s.current(), null);
  s.cleanup();
});

test('WeChat events are ignored', () => {
  const s = setup();
  s.emit('微信', '不显示');
  assert.equal(s.current(), null);
  s.cleanup();
});
