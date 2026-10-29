import { useEffect, useRef, useState } from 'react';
import { CAT_BODY_ASSET, CAT_HEAD_ASSET, type CatController, type ContinuousPose } from '../cat-controller';
import { CatHeadRenderer } from '../cat-head-renderer';

export function CharacterPortrait({ controller }: { controller: CatController }) {
  const headRef = useRef<HTMLImageElement>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const debugRef = useRef<HTMLOutputElement>(null);
  const rendererRef = useRef<CatHeadRenderer | null>(null);
  const [mesh, setMesh] = useState(false);
  const [available, setAvailable] = useState<ReadonlyMap<string, string> | null>(null);
  useEffect(() => {
    let disposed = false;
    const abort = new AbortController();
    const urls = new Set<string>();
    const assets = new Map<string, string>();
    const decodedImages = new Map<string, HTMLImageElement>();
    const canvas = canvasRef.current;
    let latestPose: ContinuousPose = { yaw: 0, pitch: 0, tx: 0, ty: 0, roll: 0 };
    const createRenderer = () => {
      if (!canvas || disposed || !assets.has(CAT_HEAD_ASSET)) return;
      rendererRef.current?.dispose(); rendererRef.current = null;
      try {
        const renderer = new CatHeadRenderer(canvas, decodedImages.get(CAT_HEAD_ASSET)!);
        rendererRef.current = renderer;
        const box = stageRef.current!.getBoundingClientRect(); renderer.resize(box.width, box.height);
        renderer.render(latestPose); delete canvas.dataset.rendererError; setMesh(true);
      } catch (error) { canvas.dataset.rendererError = String(error); setMesh(false); }
    };
    const lost = (event: Event) => {
      event.preventDefault(); rendererRef.current?.dispose(); rendererRef.current = null; setMesh(false);
    };
    canvas?.addEventListener('webglcontextlost', lost);
    canvas?.addEventListener('webglcontextrestored', createRenderer);
    // Keep decoded images alive. Blob URLs avoid repeated Tauri protocol reads
    // (which produced empty head frames despite ordinary Image preloading).
    void Promise.allSettled([CAT_BODY_ASSET, CAT_HEAD_ASSET].map(async source => {
      const response = await fetch(source, { signal: abort.signal });
      if (!response.ok) throw new Error('Cat asset unavailable');
      const blob = await response.blob();
      if (disposed) return;
      const url = URL.createObjectURL(blob);
      urls.add(url);
      const image = new Image();
      decodedImages.set(source, image);
      image.decoding = 'sync';
      image.src = url;
      await image.decode();
      if (!disposed) assets.set(source, url);
    })).then(() => { if (!disposed) { setAvailable(assets); createRenderer(); } });
    let debugTime = 0, previousTime = performance.now();
    controller.start((pose, motion) => {
      latestPose = pose;
      rendererRef.current?.render(pose);
      // Fallback only. A lost GPU context must not make the character vanish.
      if (!rendererRef.current && headRef.current) headRef.current.style.transform = `translate3d(${pose.tx}px,${pose.ty}px,0) rotateX(${-pose.pitch}deg) rotateY(${pose.yaw}deg) rotateZ(${pose.roll}deg)`;
      const stage = stageRef.current;
      if (stage) { stage.dataset.yaw = pose.yaw.toFixed(3); stage.dataset.pitch = pose.pitch.toFixed(3); }
      const now = performance.now();
      if (debugRef.current && now - debugTime > 100) {
        debugRef.current.textContent = `Target ${motion.targetX.toFixed(2)}, ${motion.targetY.toFixed(2)}\nCurrent ${motion.currentX.toFixed(2)}, ${motion.currentY.toFixed(2)}\nVelocity ${motion.velocityX.toFixed(2)}, ${motion.velocityY.toFixed(2)}\nYaw ${pose.yaw.toFixed(1)} / Pitch ${pose.pitch.toFixed(1)}\nFPS ~${Math.round(1000 / Math.max(1, now - previousTime))}`;
        debugTime = now;
      }
      previousTime = now;
    });
    return () => {
      disposed = true;
      abort.abort();
      controller.dispose();
      canvas?.removeEventListener('webglcontextlost', lost);
      canvas?.removeEventListener('webglcontextrestored', createRenderer);
      rendererRef.current?.dispose(); rendererRef.current = null;
      for (const url of urls) URL.revokeObjectURL(url);
      decodedImages.clear();
      assets.clear();
      urls.clear();
    };
  }, [controller]);
  useEffect(() => {
    const head = stageRef.current;
    if (!head) return;
    const measure = () => {
      const box = head.getBoundingClientRect();
      // Image canvases include transparent margin; face center is near 44% high.
      controller.setGeometry(box.x + box.width / 2, box.y + box.height * 0.44, window.innerWidth, window.innerHeight);
      rendererRef.current?.resize(box.width, box.height);
    };
    const move = (event: PointerEvent) => { if (event.pointerType !== 'touch') controller.onPointerMove(event.clientX, event.clientY); };
    const leave = () => controller.onPointerLeave();
    const visibility = () => { if (document.hidden) leave(); else measure(); };
    const observer = new ResizeObserver(measure);
    observer.observe(head);
    measure();
    window.addEventListener('pointermove', move, { passive: true });
    document.documentElement.addEventListener('pointerleave', leave);
    window.addEventListener('pointerleave', leave);
    window.addEventListener('blur', leave);
    window.addEventListener('resize', measure);
    document.addEventListener('visibilitychange', visibility);
    return () => {
      observer.disconnect();
      window.removeEventListener('pointermove', move);
      document.documentElement.removeEventListener('pointerleave', leave);
      window.removeEventListener('pointerleave', leave);
      window.removeEventListener('blur', leave);
      window.removeEventListener('resize', measure);
      document.removeEventListener('visibilitychange', visibility);
    };
  }, [controller]);
  const ready = available?.has(CAT_BODY_ASSET) && available.has(CAT_HEAD_ASSET);
  const headAsset = available?.get(CAT_HEAD_ASSET);
  return <div className="character-portrait cat-character"
    data-cat-renderer={mesh ? 'mesh-2.5d' : 'css-fallback'}
    data-cat-ready={Boolean(ready)} aria-hidden="true">
    <img className="cat-body-layer" src={available?.get(CAT_BODY_ASSET)} alt="" draggable={false} decoding="sync" />
    <div ref={stageRef} className="cat-head-stage">
      <img ref={headRef} className="cat-head-layer" src={headAsset} alt="" draggable={false} decoding="sync" />
      <canvas ref={canvasRef} className="cat-head-mesh" />
    </div>
    {import.meta.env.DEV && <output ref={debugRef} className="cat-debug" hidden={!new URLSearchParams(window.location.search).has('catDebug')} />}
  </div>;
}
