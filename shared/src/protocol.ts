/** Wire protocol. JSON for now; swap encode/decode for a binary format later. */

export const Button = {
  Jump: 1 << 0,
  Sprint: 1 << 1,
  Fire: 1 << 2,
  Dash: 1 << 3,
  Reload: 1 << 4,
} as const;

/** One tick of player intent. The client never sends positions. */
export interface InputFrame {
  seq: number;
  moveX: number; // -1..1 strafe
  moveZ: number; // -1..1 forward
  yaw: number;
  pitch: number;
  buttons: number; // bitmask of Button
}

export interface PlayerState {
  id: number;
  x: number;
  y: number;
  z: number;
  vy: number;
  yaw: number;
  pitch: number;
  grounded: boolean;
}

export type ClientMessage =
  | { t: 'join'; name: string; room?: string }
  | { t: 'input'; frames: InputFrame[] };

export type ServerMessage =
  | { t: 'welcome'; id: number; room: string; tickRate: number }
  | { t: 'snapshot'; tick: number; ack: number; players: PlayerState[] }
  | { t: 'left'; id: number };

export const encode = (m: ClientMessage | ServerMessage): string => JSON.stringify(m);
export const decodeClient = (raw: string): ClientMessage => JSON.parse(raw);
export const decodeServer = (raw: string): ServerMessage => JSON.parse(raw);
