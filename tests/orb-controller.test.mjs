import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
const source = readFileSync(new URL('../src/orb-controller.ts', import.meta.url), 'utf8');
const { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ESNext } });
const { createOrbController } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString('base64')}`);
const tick = () => new Promise(resolve => setImmediate(resolve));

test('hover delay, cancellation, re-entry and disposal', async () => {
  const timers = new Map(); let id = 0; const changes = [];
  const c = createOrbController(async () => {}, x => changes.push(x), assert.fail,
    (fn, ms) => { timers.set(++id, { fn, ms }); return id; }, key => timers.delete(key));
  c.request(true, 150);
  assert.equal([...timers.values()][0].ms, 150);
  c.cancelPending(); assert.equal(timers.size, 0);
  c.request(true); await tick();
  c.request(false, 400); assert.equal([...timers.values()][0].ms, 400);
  c.request(true); assert.equal(timers.size, 0);
  c.request(false, 400); c.dispose(); assert.equal(timers.size, 0);
  assert.deepEqual(changes, [true]);
});
test('enter and leave change state only after their timers fire', async () => {
  const changes = []; let scheduled;
  const c = createOrbController(async () => {}, x => changes.push(x), assert.fail,
    (fn, ms) => { scheduled = { fn, ms }; return 1; }, () => {});
  c.enter(); assert.equal(scheduled.ms, 150); assert.deepEqual(changes, []);
  scheduled.fn(); await tick(); assert.deepEqual(changes, [true]);
  c.leave(); assert.equal(scheduled.ms, 400); assert.deepEqual(changes, [true]);
  scheduled.fn(); await tick(); assert.deepEqual(changes, [true, false]);
});
test('opposite intent during resize is serialized', async () => {
  const pending = []; const changes = [];
  const c = createOrbController(x => new Promise(resolve => pending.push({ x, resolve })),
    x => changes.push(x), assert.fail);
  c.request(true); c.request(false);
  assert.equal(pending.length, 1);
  pending[0].resolve(); await tick();
  assert.equal(pending[1].x, false);
  pending[1].resolve(); await tick();
  assert.deepEqual(changes, [true, false]);
});
test('resize error preserves view and permits retry', async () => {
  let fail = true; let errors = 0; const changes = [];
  const c = createOrbController(async () => { if (fail) throw Error(); },
    x => changes.push(x), () => errors++);
  c.request(true); await tick(); assert.equal(errors, 1); assert.deepEqual(changes, []);
  fail = false; c.request(true); await tick(); assert.deepEqual(changes, [true]);
});
test('dispose suppresses late resize callbacks', async () => {
  let resolve; const changes = [];
  const c = createOrbController(() => new Promise(r => { resolve = r; }), x => changes.push(x), assert.fail);
  c.request(true); c.dispose(); resolve(); await tick(); assert.deepEqual(changes, []);
});
test('pin keeps expanded after leave; unpin and Escape release it', async () => {
  const changes = []; const timers = [];
  const c = createOrbController(async () => {}, x => changes.push(x), assert.fail,
    fn => { timers.push(fn); return timers.length; }, () => {});
  assert.equal(c.togglePin(), true); await tick(); c.leave(); assert.equal(timers.length, 0);
  assert.equal(c.togglePin(), false); c.leave(); timers.pop()(); await tick();
  assert.deepEqual(changes, [true, false]);
  c.togglePin(); await tick(); c.escape(); await tick();
  assert.deepEqual(changes, [true, false, true, false]);
});
