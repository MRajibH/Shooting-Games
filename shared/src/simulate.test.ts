import { describe, expect, it } from 'vitest';
import { Button, TICK_DT, spawnState, stepPlayer, type InputFrame } from './index';

const input = (o: Partial<InputFrame> = {}): InputFrame => ({
  seq: 0, moveX: 0, moveZ: 0, yaw: 0, pitch: 0, buttons: 0, ...o,
});

describe('stepPlayer', () => {
  it('is deterministic', () => {
    const a = spawnState(1);
    const b = spawnState(1);
    for (let i = 0; i < 90; i++) {
      const f = input({ moveZ: 1, moveX: 0.5, yaw: i * 0.05, buttons: i === 10 ? Button.Jump : 0 });
      stepPlayer(a, f, TICK_DT);
      stepPlayer(b, f, TICK_DT);
    }
    expect(a).toEqual(b);
  });

  it('moves forward along -Z at yaw 0', () => {
    const p = spawnState(1);
    stepPlayer(p, input({ moveZ: 1 }), TICK_DT);
    expect(p.z).toBeLessThan(0);
    expect(p.x).toBeCloseTo(0);
  });

  it('jumps and lands', () => {
    const p = spawnState(1);
    stepPlayer(p, input({ buttons: Button.Jump }), TICK_DT);
    expect(p.grounded).toBe(false);
    for (let i = 0; i < 60; i++) stepPlayer(p, input(), TICK_DT);
    expect(p.grounded).toBe(true);
    expect(p.y).toBe(0);
  });
});
