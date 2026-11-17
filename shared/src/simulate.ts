import { MOVE } from './constants';
import { Button, type InputFrame, type PlayerState } from './protocol';

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/**
 * Advance one player by one tick. Deterministic and side-effect free apart from
 * mutating `p`: the server calls it for authority, the client for prediction.
 * Milestone 1: flat ground only. Map collision arrives with physics.ts.
 */
export function stepPlayer(p: PlayerState, input: InputFrame, dt: number): void {
  const mx = clamp(input.moveX, -1, 1);
  const mz = clamp(input.moveZ, -1, 1);
  const len = Math.hypot(mx, mz);
  const speed = input.buttons & Button.Sprint ? MOVE.sprintSpeed : MOVE.walkSpeed;
  const scale = len > 1 ? speed / len : speed;

  p.yaw = input.yaw;
  p.pitch = clamp(input.pitch, -1.5, 1.5);
  const sin = Math.sin(p.yaw);
  const cos = Math.cos(p.yaw);
  // forward is -Z at yaw 0 (three.js convention)
  p.x += (mx * cos - mz * sin) * scale * dt;
  p.z += (-mx * sin - mz * cos) * scale * dt;

  if (p.grounded && input.buttons & Button.Jump) {
    p.vy = MOVE.jumpSpeed;
    p.grounded = false;
  }
  p.vy -= MOVE.gravity * dt;
  p.y += p.vy * dt;
  if (p.y <= 0) {
    p.y = 0;
    p.vy = 0;
    p.grounded = true;
  }
}

export function spawnState(id: number): PlayerState {
  return { id, x: 0, y: 0, z: 0, vy: 0, yaw: 0, pitch: 0, grounded: true };
}
