import assert from 'node:assert/strict';
import {readFileSync,existsSync,readdirSync} from 'node:fs';
import test from 'node:test';
import ts from 'typescript';
import {createHash} from 'node:crypto';
const hash=s=>createHash('sha256').update(s).digest('hex');
function setup(){
 let now=0,id=0;const frames=new Map(),module={exports:{}};
 const source=readFileSync('src/cat-controller.ts','utf8');
 const code=ts.transpileModule(source,{compilerOptions:{module:ts.ModuleKind.CommonJS,target:ts.ScriptTarget.ES2022}}).outputText;
 new Function('module','exports','performance','requestAnimationFrame','cancelAnimationFrame','Math',code)(module,module.exports,{now:()=>now},fn=>{frames.set(++id,fn);return id;},id=>frames.delete(id),Object.assign(Object.create(Math),{random:()=>.5}));
 const api=module.exports,controller=new api.CatController(),poses=[];
 controller.start(p=>poses.push(p));
 controller.setGeometry(400,200,800,500);
 const advance=(ms,step=1000/60)=>{const end=now+ms;while(now<end){now=Math.min(end,now+step);const callbacks=[...frames.values()];frames.clear();callbacks.forEach(fn=>fn(now));}};
 return {api,controller,poses,frames,advance,move:(x,y)=>controller.onPointerMove(400+x*320,200+y*175)};
}
test('continuous input and nonlinear center response have no direction bins',()=>{const s=setup();assert.ok(s.api.responseCurve(.2)<.2);assert.equal(s.api.responseCurve(2),1);s.move(.01,0);assert.ok(s.controller.motion.targetX>0);assert.ok(!('frameKey' in s.controller.motion));});
test('critically damped spring preserves inertia and settles without oscillation',()=>{const s=setup();s.move(1,0);s.advance(100);assert.ok(s.controller.motion.currentX>.2&&s.controller.motion.currentX<.5);s.advance(400);assert.ok(s.controller.motion.currentX>.98);assert.ok(s.poses.every(p=>p.yaw<=14));s.move(-1,0);const before=s.controller.motion.currentX;assert.equal(s.controller.motion.currentX,before);s.advance(600);assert.ok(s.controller.motion.currentX<-.98);});
test('leave keeps velocity/current; 2% center tolerance reached in 600-850ms',()=>{const s=setup();s.move(1,0);s.advance(2000);const before={...s.controller.motion};s.controller.onPointerLeave();assert.equal(s.controller.motion.currentX,before.currentX);assert.equal(s.controller.motion.velocityX,before.velocityX);s.advance(600);assert.ok(s.controller.motion.currentX>.02);s.advance(200);assert.ok(s.controller.motion.currentX<.02);});
test('same spring response at 60 and 144Hz',()=>{const a=setup(),b=setup();a.move(1,-1);b.move(1,-1);a.advance(450,1000/60);b.advance(450,1000/144);assert.ok(Math.abs(a.controller.motion.currentX-b.controller.motion.currentX)<1e-10);});
test('asymmetric pitch and bounded yaw/translation/roll',()=>{const s=setup(),p=s.api.mapPose(1,-1);assert.equal(p.yaw,14);assert.equal(p.pitch,8);assert.equal(s.api.mapPose(-1,1).pitch,-7);assert.equal(p.tx,4);assert.equal(p.ty,-3);assert.ok(Math.abs(p.roll)<=1.2);});
test('V3.1 trajectory is numerically identical to accepted V3 for slow, circular, fast and leave input',()=>{
 // Baseline captured from the accepted V3 before removing its expression code.
 // Store a fingerprint so this regression test also works without local backups.
 const current=setup(),trace=[];assert.deepEqual(current.api.MOTION,{followOmega:12,leaveOmega:8,curve:1.25,yaw:14,pitchUp:8,pitchDown:7,tx:4,ty:3});
 for(let i=0;i<500;i++){
  if(i===350||i===410)current.controller.onPointerLeave();
  else if(i<350||i>450){const x=i<100?i/50-1:i<280?Math.cos(i*.08):(i%2?1:-1),y=i<100?0:Math.sin(i*.08);current.move(x,y);}
  current.advance(1000/60);trace.push([{...current.controller.motion},{...current.poses.at(-1)}]);
 }
 // Different V8 releases may vary in the last few floating-point bits.
 // Quantize well below visible motion before comparing the accepted trace.
 const stableTrace=JSON.stringify(trace,(_key,value)=>typeof value==='number'?Number(value.toFixed(7)):value);
 assert.equal(hash(stableTrace),'62857020bb6d313f90057d1b725cb57d2b6b615d42fbf0a10f4e25128df23b2a');
});
test('controller exposes no expression, quota, refresh or hover API',()=>{const s=setup();for(const name of ['setQuota','setReduced','enter','leave','clicked','refreshStart','refreshEnd','activity','setExpression'])assert.equal(typeof s.controller[name],'undefined');});
test('idle beyond 10 seconds has no autonomous pose or face changes',()=>{const s=setup();s.move(.4,-.3);s.advance(3000);const before=s.poses.at(-1);s.advance(12000);const after=s.poses.at(-1);for(const key of Object.keys(before))assert.ok(Math.abs(after[key]-before[key])<1e-10);assert.equal(s.frames.size,1);});
test('pose callback has no state subscription; dispose cancels sole RAF',()=>{const s=setup();s.move(1,0);s.advance(1000);assert.ok(s.poses.length>=60);s.controller.dispose();assert.equal(s.frames.size,0);const count=s.poses.length;s.advance(1000);assert.equal(s.poses.length,count);s.controller.start(()=>{});assert.equal(s.frames.size,1);s.controller.dispose();});
test('only fixed body and neutral face remain in production assets',()=>{const s=setup();for(const a of [s.api.CAT_BODY_ASSET,s.api.CAT_HEAD_ASSET])assert.ok(existsSync('public'+a));assert.equal(readdirSync('public/assets/cat',{recursive:true}).filter(p=>p.endsWith('.png')).length,2);});
test('no expression runtime, random timer, settings or context subscription in cat modules',()=>{for(const file of ['src/cat-controller.ts','src/cat-head-renderer.ts','src/components/CharacterPortrait.tsx']){const s=readFileSync(file,'utf8');assert.doesNotMatch(s,/CatExpression|CatContext|expression|blink|warning|success|thinking|sleepy|setTimeout|setInterval|Math.random|useAppSettings|CAT_TARGETS/);}const s=readFileSync('src/components/CharacterPortrait.tsx','utf8');assert.doesNotMatch(s,/pointerover|pointerout|focusin|focusout|surfaceRef|quota/);});
test('accepted mesh shaders are byte-identical and GPU resources still cleaned',()=>{const current=readFileSync('src/cat-head-renderer.ts','utf8');for(const [name,expected] of Object.entries({vertexSource:'d6d68165b9ba9730c545f6a1946b7f55015e1511703eb54e009394e06f3b1f61',fragmentSource:'0775ea345b1dfd90964fd9e35b92ff6b6e34b3ae7b54d90adb3e0b8e5f53f0b0'})){const pattern=new RegExp('const '+name+' = `([\\s\\S]*?)`;');assert.equal(hash(current.match(pattern)[1]),expected);}assert.match(current,/deleteTexture/);assert.match(current,/deleteBuffer/);assert.match(current,/deleteProgram/);const component=readFileSync('src/components/CharacterPortrait.tsx','utf8');assert.match(component,/removeEventListener\('pointermove'/);assert.match(component,/observer.disconnect/);assert.match(component,/URL.revokeObjectURL/);});
test('refresh business source is identical after deleting only cat feedback hooks',()=>{
 const current=readFileSync('src/components/RefreshAction.tsx','utf8');assert.equal(hash(current.replace(/\s/g,'')),'457c234f064bc7dcaf767ddcd57ccd21e4ea544589529c8ff1201e7d2f44145a');
});
