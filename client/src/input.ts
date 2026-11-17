import { Button, type InputFrame } from './protocol';

/** Keyboard + mouse; touch controls plug into the same InputFrame later. */
export class Input {
  private keys = new Set<string>();
  private mouse = 0;
  yaw = 0;
  pitch = 0;
  sensitivity = 0.0022;
  private seq = 0;

  constructor(canvas: HTMLCanvasElement) {
    addEventListener('keydown', (e) => this.keys.add(e.code));
    addEventListener('keyup', (e) => this.keys.delete(e.code));
    addEventListener('blur', () => { this.keys.clear(); this.mouse = 0; });
    canvas.addEventListener('click', () => canvas.requestPointerLock());
    canvas.addEventListener('contextmenu', (e) => e.preventDefault());
    addEventListener('mousedown', (e) => (this.mouse |= 1 << e.button));
    addEventListener('mouseup', (e) => (this.mouse &= ~(1 << e.button)));
    addEventListener('mousemove', (e) => {
      if (document.pointerLockElement !== canvas) return;
      const s = this.sensitivity * (this.aiming ? 0.55 : 1);
      this.yaw -= e.movementX * s;
      this.pitch = Math.max(-1.45, Math.min(1.45, this.pitch - e.movementY * s));
    });
  }

  get aiming() { return (this.mouse & 4) !== 0; }

  sample(): InputFrame {
    const k = (c: string) => (this.keys.has(c) ? 1 : 0);
    let buttons = 0;
    if (k('Space')) buttons |= Button.Jump;
    if (k('ShiftLeft')) buttons |= Button.Sprint;
    if (k('ControlLeft') || k('KeyC')) buttons |= Button.Crouch;
    if (k('KeyR')) buttons |= Button.Reload;
    if (this.mouse & 1) buttons |= Button.Fire;
    if (this.aiming) buttons |= Button.Aim;
    return {
      seq: ++this.seq,
      moveX: k('KeyD') - k('KeyA'),
      moveZ: k('KeyW') - k('KeyS'),
      yaw: this.yaw,
      pitch: this.pitch,
      buttons,
    };
  }
}
