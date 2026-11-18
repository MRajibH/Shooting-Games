import type { Input } from './input';
import { Button } from './protocol';

/**
 * True on phones and tablets: the primary pointer is a finger. Touchscreen laptops
 * with a mouse keep mouse controls. `?touch=1` / `?touch=0` overrides.
 */
export function isTouchDevice() {
  const force = new URLSearchParams(location.search).get('touch');
  if (force === '1' || force === '0') return force === '1';
  return matchMedia('(pointer: coarse)').matches;
}

const STICK_RADIUS = 56;

/** Keep receiving a finger's moves even when it slides off the element. */
function capture(el: HTMLElement, id: number) {
  try { el.setPointerCapture(id); } catch { /* pointer already gone */ }
}

/**
 * Twin-stick touch controls: a floating movement stick on the left (push to the
 * top edge to sprint), drag anywhere else to look, and buttons on the right for
 * fire (drag on it to keep aiming), aim, jump, crouch, reload and weapon swap.
 */
export class TouchControls {
  readonly root: HTMLElement;
  private held = new Set<number>();
  private toggles = 0;
  private stickBase: HTMLElement;
  private stickKnob: HTMLElement;
  private fireBtn: HTMLElement;
  onScore: () => void = () => {};

  constructor(private input: Input) {
    const root = document.createElement('div');
    root.id = 'touch';
    root.innerHTML = `
      <div class="zone stick-zone"></div>
      <div class="zone look-zone"></div>
      <div class="stick-base hidden"><div class="stick-knob"></div></div>
      <button class="tb fire" data-b="fire">FIRE</button>
      <button class="tb aim" data-t="aim">AIM</button>
      <button class="tb jump" data-b="jump">JUMP</button>
      <button class="tb crouch" data-t="crouch">CROUCH</button>
      <button class="tb reload" data-tap="reload">R</button>
      <button class="tb swap" data-swap>SWAP</button>
      <button class="tb lean-l" data-b="leanl">Q</button>
      <button class="tb lean-r" data-b="leanr">E</button>
      <button class="tb small score">SCORE</button>`;
    document.body.append(root);
    this.root = root;
    this.stickBase = root.querySelector('.stick-base')!;
    this.stickKnob = root.querySelector('.stick-knob')!;
    this.fireBtn = root.querySelector('.fire')!;
    this.wireStick(root.querySelector('.stick-zone')!);
    this.wireLook(root.querySelector('.look-zone')!);
    const bits: Record<string, number> = {
      fire: Button.Fire, jump: Button.Jump, aim: Button.Aim, crouch: Button.Crouch,
      reload: Button.Reload, leanl: Button.LeanLeft, leanr: Button.LeanRight,
    };
    for (const el of Array.from(root.querySelectorAll<HTMLElement>('.tb'))) {
      el.addEventListener('contextmenu', (e) => e.preventDefault());
      if (el.dataset.b) {
        // held while the finger is down; dragging on it still turns the view
        const bit = bits[el.dataset.b]!;
        let last: [number, number] | null = null;
        el.addEventListener('pointerdown', (e) => {
          capture(el, e.pointerId);
          this.held.add(bit);
          el.classList.add('on');
          last = [e.clientX, e.clientY];
          this.sync();
        });
        el.addEventListener('pointermove', (e) => {
          if (!last) return;
          this.input.look(e.clientX - last[0], e.clientY - last[1]);
          last = [e.clientX, e.clientY];
        });
        const up = () => { this.held.delete(bit); el.classList.remove('on'); last = null; this.sync(); };
        el.addEventListener('pointerup', up);
        el.addEventListener('pointercancel', up);
      } else if (el.dataset.t) {
        const bit = bits[el.dataset.t]!;
        el.addEventListener('pointerdown', () => {
          this.toggles ^= bit;
          el.classList.toggle('on', (this.toggles & bit) !== 0);
          this.sync();
        });
      } else if (el.dataset.tap) {
        const bit = bits[el.dataset.tap]!;
        el.addEventListener('pointerdown', () => {
          this.held.add(bit);
          this.sync();
          setTimeout(() => { this.held.delete(bit); this.sync(); }, 150);
        });
      } else if ('swap' in el.dataset) {
        el.addEventListener('pointerdown', () => { this.input.weapon = (this.input.weapon + 1) % 4; });
      } else if (el.classList.contains('score')) {
        el.addEventListener('pointerdown', () => this.onScore());
      }
    }
  }

  /** Slap mode has no guns: the fire button slaps and the gun buttons go away. */
  setArmed(armed: boolean) {
    this.fireBtn.textContent = armed ? 'FIRE' : 'SLAP';
    this.root.classList.toggle('unarmed', !armed);
  }

  set visible(v: boolean) { this.root.classList.toggle('hidden', !v); }

  private sync() {
    let b = this.toggles;
    for (const h of this.held) b |= h;
    // sprint comes from the stick, not a button
    this.input.touch.buttons = b | (this.input.touch.buttons & Button.Sprint);
  }

  private wireStick(zone: HTMLElement) {
    let id = -1;
    let ox = 0, oy = 0;
    zone.addEventListener('pointerdown', (e) => {
      if (id !== -1) return;
      id = e.pointerId;
      capture(zone, id);
      ox = e.clientX;
      oy = e.clientY;
      this.stickBase.style.transform = `translate(${ox}px, ${oy}px)`;
      this.stickKnob.style.transform = 'translate(0px, 0px)';
      this.stickBase.classList.remove('hidden');
    });
    zone.addEventListener('pointermove', (e) => {
      if (e.pointerId !== id) return;
      let dx = e.clientX - ox, dy = e.clientY - oy;
      const len = Math.hypot(dx, dy);
      if (len > STICK_RADIUS) { dx *= STICK_RADIUS / len; dy *= STICK_RADIUS / len; }
      this.stickKnob.style.transform = `translate(${dx}px, ${dy}px)`;
      const t = this.input.touch;
      const dead = (v: number) => (Math.abs(v) < 0.12 ? 0 : v);
      t.mx = dead(dx / STICK_RADIUS);
      t.mz = dead(-dy / STICK_RADIUS);
      // pushed all the way forward: sprint
      const sprint = len > STICK_RADIUS * 1.15 && t.mz > 0.7;
      t.buttons = sprint ? t.buttons | Button.Sprint : t.buttons & ~Button.Sprint;
    });
    const up = (e: PointerEvent) => {
      if (e.pointerId !== id) return;
      id = -1;
      const t = this.input.touch;
      t.mx = 0;
      t.mz = 0;
      t.buttons &= ~Button.Sprint;
      this.stickBase.classList.add('hidden');
    };
    zone.addEventListener('pointerup', up);
    zone.addEventListener('pointercancel', up);
  }

  private wireLook(zone: HTMLElement) {
    const last = new Map<number, [number, number]>();
    zone.addEventListener('pointerdown', (e) => {
      capture(zone, e.pointerId);
      last.set(e.pointerId, [e.clientX, e.clientY]);
    });
    zone.addEventListener('pointermove', (e) => {
      const l = last.get(e.pointerId);
      if (!l) return;
      this.input.look(e.clientX - l[0], e.clientY - l[1]);
      last.set(e.pointerId, [e.clientX, e.clientY]);
    });
    const up = (e: PointerEvent) => last.delete(e.pointerId);
    zone.addEventListener('pointerup', up);
    zone.addEventListener('pointercancel', up);
  }
}
