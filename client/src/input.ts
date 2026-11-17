import { Button, type InputFrame } from '@riftline/shared';

/** Keyboard + mouse for now; touch controls plug into the same InputFrame later. */
export class Input {
  private keys = new Set<string>();
  yaw = 0;
  pitch = 0;
  private seq = 0;

  constructor(canvas: HTMLCanvasElement) {
    addEventListener('keydown', (e) => this.keys.add(e.code));
    addEventListener('keyup', (e) => this.keys.delete(e.code));
    addEventListener('blur', () => this.keys.clear());
    canvas.addEventListener('click', () => canvas.requestPointerLock());
    addEventListener('mousemove', (e) => {
      if (document.pointerLockElement !== canvas) return;
      this.yaw -= e.movementX * 0.0025;
      this.pitch = Math.max(-1.4, Math.min(1.4, this.pitch - e.movementY * 0.0025));
    });
  }

  sample(): InputFrame {
    const k = (c: string) => (this.keys.has(c) ? 1 : 0);
    let buttons = 0;
    if (k('Space')) buttons |= Button.Jump;
    if (k('ShiftLeft')) buttons |= Button.Sprint;
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
