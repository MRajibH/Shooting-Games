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
  FireMode: 1 << 8,
} as const;

export interface InputFrame {
  seq: number;
  moveX: number;
  moveZ: number;
  yaw: number;
  pitch: number;
  buttons: number;
  /** Weapon slot wanted (0..3). */
  weapon: number;
  /** Server tick of the world on screen when this frame was made (lag compensation). */
  view: number;
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
  /** 0 free-for-all, else 1 or 2. */
  team: number;
  armed: boolean;
  hp: number;
  dead: number;
  regenCd: number;
  weapon: number;
  mags: number[];
  reserves: number[];
  fireCd: number;
  reload: number;
  reloadLen: number;
  swap: number;
  lowered: number;
  ads: number;
  bloom: number;
  shots: number;
  semi: boolean;
  triggerSpent: boolean;
  held: number;
}

export type Zone = 'head' | 'torso' | 'limb';
export type V3 = [number, number, number];

export interface WeaponSpec {
  name: string; mag: number; reserve: number; interval: number; auto: boolean; pellets: number;
  muzzleVelocity: number; drag: number; damage: number; falloffStart: number; falloffEnd: number;
  falloffMin: number; headMult: number; limbMult: number; hipSpread: number; adsSpread: number;
  moveSpread: number; bloom: number; kickUp: number; kickSide: number; reloadTactical: number;
  reloadEmpty: number; reloadPerShell: boolean; chamber: boolean; adsTime: number; adsFov: number; sway: number;
}

export type Mode = 'combat' | 'tdm' | 'domination' | 'slap';
export type Difficulty = 'easy' | 'normal' | 'hard';

export interface FlagState { x: number; z: number; owner: number; progress: number }

export interface MatchState {
  phase: 'playing' | 'ended';
  timeLeft: number;
  limit: number;
  scores: [number, number];
  flags: FlagState[];
  winnerTeam: number;
  winnerId: number;
  mvp: number;
  nextIn: number;
}

export type GameEvent =
  | { t: 'slap'; from: number; to: number | null }
  | { t: 'ko'; from: number; to: number }
  | { t: 'respawn'; id: number }
  | { t: 'fire'; id: number; w: number; o: V3; d: V3 }
  | { t: 'impact'; p: V3; n: V3; mat: number }
  | { t: 'hit'; from: number; to: number; dmg: number; zone: Zone; p: V3 }
  | { t: 'kill'; from: number; to: number; w: number; head: boolean; dist: number }
  | { t: 'capture'; flag: number; team: number };

export interface RosterEntry {
  id: number; name: string; slaps: number; kos: number; kills: number; deaths: number;
  caps: number; score: number; team: number; bot: boolean;
}

export type ClientMessage =
  | { t: 'join'; name: string; room?: string; mode?: Mode; difficulty?: Difficulty }
  | { t: 'input'; frames: InputFrame[] };

export type ServerMessage =
  | { t: 'welcome'; id: number; room: string; tickRate: number; mode: Mode; difficulty: Difficulty }
  | { t: 'snapshot'; tick: number; ack: number; players: PlayerState[]; events: GameEvent[]; game: MatchState }
  | { t: 'roster'; players: RosterEntry[] }
  | { t: 'left'; id: number };
