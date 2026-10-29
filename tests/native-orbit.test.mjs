import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import test from 'node:test';
const shader=readFileSync(new URL('../src-tauri/native-orb/native_orb_shader.hlsl',import.meta.url),'utf8');
const extract=name=>shader.match(new RegExp(`float ${name}\\([^)]*\\)\\s*\\{([^}]+)\\}`))[1];
const phase=new Function('time','frac','min',extract('headPhase'));
const sample=t=>phase(t,x=>x-Math.floor(x),Math.min);
const fade=new Function('remaining','pow','saturate',extract('trailAlpha'));
const alpha=x=>fade(x,Math.pow,x=>Math.max(0,Math.min(1,x)));
const near=(a,b)=>assert.ok(Math.abs(a-b)<1e-9);
test('2.4s orbit has 2.16s linear movement and .24s hold',()=>{
 near(sample(1.08),.5);near(sample(2.16),1);near(sample(2.39),1);near(sample(2.4),0);
 for(const t of [.1,.5,1,2])near(sample(t),sample(t+2.4));
});
test('single analytic 36-degree trail fades continuously and tapers',()=>{
 near(alpha(0),0);near(alpha(1),.62);
 for(const [u,a] of [[.25,.12],[.5,.28],[.75,.45]])assert.ok(Math.abs(alpha(u)-a)<.01);
 let last=0;for(let i=0;i<=1000;i++){const a=alpha(i/1000);assert.ok(a>=last&&a-last<.001);last=a;}
 assert.match(shader,/trailAngle = 0\.628318530718/);
 assert.match(shader,/lerp\(0\.5, 2\.4, remaining\)/);
 assert.match(shader,/frac\(\(theta - angle\) \/ 6\.28318530718\)/);
 assert.doesNotMatch(shader,/sparkBirth|streak|weights\[|cometLight|for \(/);
});
test('same-hue overlay keeps progress and text owners; only outer shell is dimmed',()=>{
 for(const c of ['x','y','z'])assert.ok(shader.includes(`over(result, accent, masks.${c})`));
 assert.match(shader,/float emissiveIntensity = 0\.72/);
 assert.match(shader,/lerp\(1\.0, 0\.65, smoothstep\(30\.0, 34\.0, radius \* 40\.0\)\)/);
 assert.ok(shader.indexOf('orbitMasks(v')<shader.indexOf('float text = valueMask'));
});
test('4.3 DIP head, 6.5 DIP glow and .4 DIP offset reuse the clock and geometry',()=>{
 assert.match(shader,/smoothstep\(1\.75, 2\.15, headDistance\)/);
 assert.match(shader,/headDistance \/ 6\.5\), 2\.0\) \* 0\.22/);
 assert.match(shader,/pathRadius = orbitRadius \+ 0\.4/);
 assert.match(shader,/if \(time <= 0\.0\) return 0\.0/);
 const helper=shader.slice(shader.indexOf('float headPhase'),shader.indexOf('float4 PSMain'));
 assert.doesNotMatch(helper,/dpiScale|float3\(\d/);
});
