export const CAT_BODY_ASSET = '/assets/cat/body/00_body_locked.png';
export const CAT_HEAD_ASSET = '/assets/cat/heads/base/05_center.png';
export const MOTION = { followOmega: 12, leaveOmega: 8, curve: 1.25, yaw: 14, pitchUp: 8, pitchDown: 7, tx: 4, ty: 3 };
export interface MotionState {
  targetX: number; targetY: number; currentX: number; currentY: number;
  velocityX: number; velocityY: number;
}
export interface ContinuousPose { yaw: number; pitch: number; tx: number; ty: number; roll: number }
const clamp = (v: number) => Math.max(-1, Math.min(1, v));
export const responseCurve = (v: number) => Math.sign(v) * Math.pow(Math.abs(clamp(v)), MOTION.curve);
// Analytic critically damped spring (seconds), independent of display rate.
// Unlike a discrete damping multiplier, larger omega means faster convergence.
export function springStep(x: number, velocity: number, target: number, dt: number, omega: number): [number, number] {
  const offset = x - target, b = velocity + omega * offset, decay = Math.exp(-omega * dt);
  return [target + (offset + b * dt) * decay, (velocity - omega * b * dt) * decay];
}
export function mapPose(x: number, y: number): ContinuousPose {
  x = clamp(x); y = clamp(y);
  return { yaw: x * MOTION.yaw, pitch: -y * (y < 0 ? MOTION.pitchUp : MOTION.pitchDown),
    tx: x * MOTION.tx, ty: y * MOTION.ty, roll: x * -0.6 + y * 0.15 };
}

export class CatController {
  readonly motion: MotionState = { targetX: 0, targetY: 0, currentX: 0, currentY: 0, velocityX: 0, velocityY: 0 };
  private active = false;
  private raf: number | null = null;
  private lastTick = 0;
  private returning = false;
  private geometry = { x: 380, y: 200, rangeX: 304, rangeY: 164.5 };
  private animate: ((pose: ContinuousPose, motion: MotionState) => void) | null = null;
  start(animate: (pose: ContinuousPose, motion: MotionState) => void) {
    if (this.active) this.dispose();
    this.active = true; this.animate = animate;
    this.lastTick = performance.now();
    this.applyRender(); this.raf = requestAnimationFrame(this.tick);
  }
  dispose() {
    this.active = false;
    if (this.raf !== null) cancelAnimationFrame(this.raf);
    this.raf = null; this.animate = null; this.returning = false;
    Object.assign(this.motion, { targetX: 0, targetY: 0, currentX: 0, currentY: 0, velocityX: 0, velocityY: 0 });
  }
  setGeometry(x: number, y: number, width: number, height: number) {
    this.geometry = { x, y, rangeX: Math.max(1, width * 0.4), rangeY: Math.max(1, height * 0.35) };
  }
  onPointerMove(x: number, y: number) {
    if (!Number.isFinite(x) || !Number.isFinite(y)) return;
    const nx = responseCurve((x - this.geometry.x) / this.geometry.rangeX);
    const ny = responseCurve((y - this.geometry.y) / this.geometry.rangeY);
    if (this.returning || Math.hypot(nx - this.motion.targetX, ny - this.motion.targetY) > 0.0001) {
      this.motion.targetX = nx; this.motion.targetY = ny; this.returning = false;
    }
  }
  onPointerLeave() {
    if (this.returning) return;
    this.returning = true; this.motion.targetX = this.motion.targetY = 0;
  }
  private applyRender() {
    if (!this.active) return;
    this.animate?.(mapPose(this.motion.currentX, this.motion.currentY), this.motion);
  }
  private tick = (time: number) => {
    if (!this.active) return;
    const dt = Math.max(0, Math.min(0.05, (time - this.lastTick) / 1000)); this.lastTick = time;
    const m = this.motion, omega = this.returning ? MOTION.leaveOmega : MOTION.followOmega;
    [m.currentX, m.velocityX] = springStep(m.currentX, m.velocityX, m.targetX, dt, omega);
    [m.currentY, m.velocityY] = springStep(m.currentY, m.velocityY, m.targetY, dt, omega);
    this.applyRender(); this.raf = requestAnimationFrame(this.tick);
  };
}
