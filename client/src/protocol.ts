// Mirror of crates/sim/src/protocol.rs. Keep the two in sync.

export const Button = {
  Jump: 1 << 0,
  Sprint: 1 << 1,
  Fire: 1 << 2,
  Aim: 1 << 3,
  Reload: 1 << 4,
  Crouch: 1 << 5,
} as const;

export interface InputFrame {
  seq: number;
  moveX: number;
  moveZ: number;
  yaw: number;
  pitch: number;
  buttons: number;
}

export interface PlayerState {
  id: number;
  x: number; y: number; z: number;
  vx: number; vy: number; vz: number;
  yaw: number;
  pitch: number;
  grounded: boolean;
  crouched: boolean;
}

export type ClientMessage =
  | { t: 'join'; name: string; room?: string }
  | { t: 'input'; frames: InputFrame[] };

export type ServerMessage =
  | { t: 'welcome'; id: number; room: string; tickRate: number }
  | { t: 'snapshot'; tick: number; ack: number; players: PlayerState[] }
  | { t: 'left'; id: number };
