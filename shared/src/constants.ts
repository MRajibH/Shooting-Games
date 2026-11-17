/** Simulation and network rates. Shared by client prediction and the server. */
export const TICK_RATE = 30;
export const TICK_DT = 1 / TICK_RATE;
export const SNAPSHOT_RATE = 20;
export const INTERP_DELAY_MS = 100;
export const MAX_REWIND_MS = 200;
export const MAX_PLAYERS = 8;
export const DEFAULT_PORT = 8080;

export const MOVE = {
  walkSpeed: 7,
  sprintSpeed: 10.5,
  gravity: 28,
  jumpSpeed: 10,
} as const;
