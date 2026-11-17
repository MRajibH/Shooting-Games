// Mirror of crates/sim/src/protocol.rs. Keep the two in sync.

export const Button = {
  Jump: 1 << 0,
  Sprint: 1 << 1,
  Fire: 1 << 2,
  Aim: 1 << 3,
  Reload: 1 << 4,
  Crouch: 1 << 5,
  LeanLeft: 1 << 6,
  LeanRight: 1 << 7,
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
  stamina: number;
  exhausted: boolean;
  lean: number;
  stun: number;
  ko: number;
  slapCd: number;
  daze: number;
}

export type Mode = 'combat' | 'slap';

export type GameEvent =
  | { t: 'slap'; from: number; to: number | null }
  | { t: 'ko'; from: number; to: number }
  | { t: 'respawn'; id: number };

export interface RosterEntry { id: number; name: string; slaps: number; kos: number }

export type ClientMessage =
  | { t: 'join'; name: string; room?: string; mode?: Mode }
  | { t: 'input'; frames: InputFrame[] };

export type ServerMessage =
  | { t: 'welcome'; id: number; room: string; tickRate: number; mode: Mode }
  | { t: 'snapshot'; tick: number; ack: number; players: PlayerState[]; events: GameEvent[] }
  | { t: 'roster'; players: RosterEntry[] }
  | { t: 'left'; id: number };
