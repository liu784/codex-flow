import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import LiquidGlass from 'liquid-glass-react';
import { Gate3Preview } from './gate3-preview';
import { glassModeForKey } from './gate3-glass-shortcut';
import './glass-prototype.css';

// Throwaway DEV-only material comparison. No layout variants, persistence or live data.
// Alt+5/F6 current, Alt+6/F7 standard; Alt+1..4 retain existing fixture controls.
// Shader is deliberately not enabled until standard passes native WebView2 acceptance.
export default function GlassPrototype() {
  const root = useRef<HTMLDivElement>(null);
  const [variant, setVariant] = useState('current');
  const [surface, setSurface] = useState<HTMLElement | null>(null);
  useEffect(() => {
    const findSurface = () => setSurface(root.current?.querySelector<HTMLElement>('.quota-panel, .floating-orb') ?? null);
    findSurface();
    const observer = new MutationObserver(findSurface);
    if (root.current) observer.observe(root.current, { childList: true, subtree: true });
    const key = (event: KeyboardEvent) => {
      const mode = glassModeForKey(event);
      if (mode === null) return;
      event.preventDefault();
      setVariant(mode);
    };
    window.addEventListener('keydown', key, true);
    return () => { observer.disconnect(); window.removeEventListener('keydown', key, true); };
  }, []);
  useEffect(() => {
    document.title = `Codex Flow — prototype / ${variant}`;
    if (import.meta.env.DEV && import.meta.env.VITE_GATE3_PREVIEW === '1') {
      console.info(`[gate3] glass mode = ${variant}`);
    }
    return () => { document.title = 'Codex Flow'; };
  }, [variant]);
  const orb = surface?.classList.contains('floating-orb') ?? true;
  return <div ref={root} className={`glass-prototype prototype-${variant}`} data-material={variant}>
    <Gate3Preview />
    {import.meta.env.DEV && import.meta.env.VITE_GATE3_PREVIEW === '1' &&
      <output className="gate3-material-badge" aria-live="polite" aria-label="Gate 3 glass mode">
        {variant.toUpperCase()}
      </output>}
    {surface && variant === 'standard' && createPortal(
      <div className="liquid-optics" aria-hidden="true">
        <LiquidGlass mode="standard" displacementScale={orb ? 18 : 8}
          blurAmount={0} saturation={125} aberrationIntensity={0.15} elasticity={0}
          cornerRadius={orb ? 23 : 30} padding="0" mouseContainer={root}
          style={{ position: 'absolute', top: '50%', left: '50%', width: '100%', height: '100%' }}>
          <span />
        </LiquidGlass>
      </div>, surface)}
  </div>;
}
