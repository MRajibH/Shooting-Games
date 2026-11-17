import { Button, type InputFrame, type WeaponSpec } from './protocol';

export interface AimContext {
  dt: number;
  spec: WeaponSpec | null;
  ads: number;
  crouched: boolean;
  speed: number;
  stamina: number;
  firing: boolean;
}

/**
 * Keyboard + mouse. Produces InputFrames whose yaw/pitch include weapon sway and
 * recoil, so the camera, the predicted shot and the server all use the same aim.
 */
export class Input {
  private keys = new Set<string>();
  private mouse = 0;
  /** Aim the player controls (mouse + recoil the player has not pulled back down). */
  yaw = 0;
  pitch = 0;
  /** Aim actually used this tick: the above plus sway. */
  viewYaw = 0;
  viewPitch = 0;
  sensitivity = 0.0022;
  weapon = 0;
  /** Seconds of breath left for steadying a scope, and lockout after running out. */
  breath = 4;
  private breathLock = 0;
  private seq = 0;
  private time = 0;
  private kickPending = 0;
  private kickYawPending = 0;
  private recoilDebt = 0;
  private sinceShot = 1;

  constructor(canvas: HTMLCanvasElement) {
    addEventListener('keydown', (e) => {
      this.keys.add(e.code);
      const n = ['Digit1', 'Digit2', 'Digit3', 'Digit4'].indexOf(e.code);
      if (n >= 0) this.weapon = n;
    });
    addEventListener('keyup', (e) => this.keys.delete(e.code));
    addEventListener('blur', () => { this.keys.clear(); this.mouse = 0; });
    canvas.addEventListener('click', () => canvas.requestPointerLock()?.catch?.(() => {}));
    canvas.addEventListener('contextmenu', (e) => e.preventDefault());
    addEventListener('mousedown', (e) => { if (document.pointerLockElement === canvas) this.mouse |= 1 << e.button; });
    addEventListener('mouseup', (e) => (this.mouse &= ~(1 << e.button)));
    addEventListener('wheel', (e) => {
      if (document.pointerLockElement !== canvas) return;
      this.weapon = (this.weapon + (e.deltaY > 0 ? 1 : 3)) % 4;
    });
    addEventListener('mousemove', (e) => {
      if (document.pointerLockElement !== canvas) return;
      const s = this.sensitivity * this.zoomScale;
      this.yaw -= e.movementX * s;
      this.pitch = Math.max(-1.45, Math.min(1.45, this.pitch - e.movementY * s));
    });
  }

  /** Mouse sensitivity scale for the current zoom (set by the camera). */
  zoomScale = 1;

  get aiming() { return (this.mouse & 4) !== 0; }
  get holdingBreath() { return this.keys.has('ShiftLeft') && this.breathLock <= 0 && this.breath > 0; }

  /** Recoil from a shot: kicks the aim up (and sideways) over the next few ticks. */
  kick(up: number, side: number) {
    this.kickPending += up;
    this.kickYawPending += side;
    this.sinceShot = 0;
  }

  sample(ctx: AimContext): InputFrame {
    const { dt } = ctx;
    this.time += dt;
    // recoil: apply the kick quickly, then let the muzzle settle back about halfway
    const k = Math.min(1, dt * 30);
    const kp = this.kickPending * k, ky = this.kickYawPending * k;
    this.kickPending -= kp;
    this.kickYawPending -= ky;
    this.pitch = Math.min(1.45, this.pitch + kp);
    this.yaw -= ky;
    this.recoilDebt += kp * 0.5;
    this.sinceShot += dt;
    if (this.sinceShot > 0.12 && this.recoilDebt > 0) {
      const back = Math.min(this.recoilDebt, this.recoilDebt * dt * 9 + 0.0005);
      this.recoilDebt -= back;
      this.pitch -= back;
    }

    // sway: grows with weapon weight, movement and fatigue; crouching and breath steady it
    let swayYaw = 0, swayPitch = 0;
    if (ctx.spec) {
      const steady = this.holdingBreath && ctx.ads > 0.5;
      if (steady) this.breath = Math.max(0, this.breath - dt);
      else this.breath = Math.min(4, this.breath + dt * 0.6);
      if (this.breath <= 0 && this.breathLock <= 0) this.breathLock = 3;
      this.breathLock = Math.max(0, this.breathLock - dt);
      const amp = ctx.spec.sway * (0.35 + 0.65 * ctx.ads) * (ctx.crouched ? 0.55 : 1) *
        (1 + ctx.speed * 0.35) * (1 + (1 - ctx.stamina) * 0.8) *
        (steady ? 0.12 : this.breathLock > 0 ? 1.8 : 1);
      const t = this.time;
      swayYaw = amp * (Math.sin(t * 0.9) * 0.7 + Math.sin(t * 2.3 + 1.3) * 0.3);
      swayPitch = amp * 0.7 * (Math.sin(t * 1.7 + 0.4) * 0.7 + Math.sin(t * 3.1) * 0.3);
    }
    this.viewYaw = this.yaw + swayYaw;
    this.viewPitch = Math.max(-1.45, Math.min(1.45, this.pitch + swayPitch));

    const key = (c: string) => (this.keys.has(c) ? 1 : 0);
    let buttons = 0;
    if (key('Space')) buttons |= Button.Jump;
    if (key('ShiftLeft')) buttons |= Button.Sprint;
    if (key('ControlLeft') || key('KeyC')) buttons |= Button.Crouch;
    if (key('KeyR')) buttons |= Button.Reload;
    if (key('KeyQ')) buttons |= Button.LeanLeft;
    if (key('KeyE')) buttons |= Button.LeanRight;
    if (key('KeyB')) buttons |= Button.FireMode;
    if (this.mouse & 1) buttons |= Button.Fire;
    if (this.aiming) buttons |= Button.Aim;
    return {
      seq: ++this.seq,
      moveX: key('KeyD') - key('KeyA'),
      moveZ: key('KeyW') - key('KeyS'),
      yaw: this.viewYaw,
      pitch: this.viewPitch,
      buttons,
      weapon: this.weapon,
      view: 0,
    };
  }
}
