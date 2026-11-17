import * as THREE from 'three';
import init, { eyeHeight, leanShift, mapBoxes, mapHalf, shoulderOffset, tickDt } from './wasm/riftline_sim.js';
import { Sfx } from './audio';
import { Character, loadCharacter } from './character';
import { Fx } from './fx';
import { spec, weapons } from './guns';
import { Input } from './input';
import { Flag, Net } from './net';
import type { GameEvent, Mode, V3 } from './protocol';
import { buildWorld, parseBoxes, raycastBoxes } from './world';

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;

await init();

const renderer = new THREE.WebGLRenderer({ antialias: true, powerPreference: 'high-performance' });
renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
renderer.toneMapping = THREE.ACESFilmicToneMapping;
renderer.toneMappingExposure = 0.55;
renderer.shadowMap.enabled = true;
renderer.shadowMap.type = THREE.PCFSoftShadowMap;
document.body.prepend(renderer.domElement);

const scene = new THREE.Scene();
const boxes = parseBoxes(new Float32Array(mapBoxes()));
const world = buildWorld(renderer, scene, boxes, mapHalf());
const camera = new THREE.PerspectiveCamera(72, 1, 0.05, 3000);
const resize = () => {
  renderer.setSize(innerWidth, innerHeight);
  camera.aspect = innerWidth / innerHeight;
  camera.updateProjectionMatrix();
};
addEventListener('resize', resize);
resize();

const charLoad = loadCharacter(`${import.meta.env.BASE_URL}models/soldier.glb`);
const sfx = new Sfx();
const fx = new Fx(scene, camera);
const input = new Input(renderer.domElement);
const SHOULDER = shoulderOffset();
const LEAN_SHIFT = leanShift();

// Menu: pre-fill from the URL (?name=&room=&mode=) and last session.
const params = new URLSearchParams(location.search);
const nameEl = $<HTMLInputElement>('name');
const roomEl = $<HTMLInputElement>('room');
try { nameEl.value = params.get('name') ?? localStorage.getItem('riftline.name') ?? ''; } catch { /* storage blocked */ }
roomEl.value = params.get('room') ?? '';
const startMode = params.get('mode');
if (startMode === 'combat' || startMode === 'slap') {
  ($<HTMLInputElement>('join').querySelector(`input[value=${startMode}]`) as HTMLInputElement).checked = true;
}

let net: Net | null = null;
let selfChar: Character | null = null;
const others = new Map<number, Character>();
const tints = [0xc9d6ff, 0xffd0b0, 0xc6ffcf, 0xffc4d8, 0xfff2b0, 0xd6c8ff, 0xb8f4ff, 0xffffff].map((c) => new THREE.Color(c));

$('join').addEventListener('submit', async (e) => {
  e.preventDefault();
  sfx.unlock();
  const name = nameEl.value.trim();
  try { localStorage.setItem('riftline.name', name); } catch { /* storage blocked */ }
  const mode = (new FormData(e.target as HTMLFormElement).get('mode') ?? 'combat') as Mode;
  await charLoad;
  $('menu').classList.add('hidden');
  $('hud').classList.remove('hidden');
  renderer.domElement.requestPointerLock()?.catch?.(() => {});
  start(name, roomEl.value.trim() || undefined, mode);
});

const esc = (s: string) => s.replace(/[&<>"]/g, (c) => `&#${c.charCodeAt(0)};`);
const v3 = (a: V3) => new THREE.Vector3(a[0], a[1], a[2]);

/** Who killed me, for the death screen. */
let deathNote = '';
/** Last hits on me: where they came from (for the damage direction arcs). */
const hurtFrom: { pos: THREE.Vector3; age: number; el: HTMLElement }[] = [];
let shake = 0;

function start(name: string, room: string | undefined, mode: Mode) {
  net = new Net(name, room, mode);
  net.onStatus = (s) => ($('status').textContent = s);
  net.onWelcome = () => {
    selfChar = new Character(scene, tints[net!.id % tints.length]);
    const combat = net!.mode === 'combat';
    $('daze-wrap').classList.toggle('hidden', combat);
    $('hp-wrap').classList.toggle('hidden', !combat);
    $('ammo').classList.toggle('hidden', !combat);
    $('help').textContent = combat
      ? '1–4 / wheel weapons · R reload · B fire mode · Right mouse aim · Shift (aimed) hold breath · Esc release mouse'
      : 'Left mouse slap · Right mouse aim · Esc release mouse';
  };
  net.onRoster = renderBoard;
  net.onSpawn = (yaw) => { input.yaw = yaw; input.pitch = 0; };
  net.onEvent = onEvent;
}

function onEvent(ev: GameEvent) {
  if (!net) return;
  const me = net.id;
  switch (ev.t) {
    case 'slap': {
      if (ev.from !== me) others.get(ev.from)?.slap();
      if (ev.to != null) {
        const [pan, gain] = spatialId(ev.to === me ? null : ev.to);
        sfx.slapHit(pan, gain);
        if (ev.from === me) flashHit('');
      } else if (ev.from !== me) {
        const [pan, gain] = spatialId(ev.from);
        sfx.whoosh(pan, gain);
      }
      break;
    }
    case 'ko': {
      const [pan, gain] = spatialId(ev.to === me ? null : ev.to);
      sfx.knockout(pan, gain);
      feed(ev.from === me ? `You knocked out <b>${esc(net.nameOf(ev.to))}</b>` :
        ev.to === me ? `<b>${esc(net.nameOf(ev.from))}</b> knocked you out` :
        `<b>${esc(net.nameOf(ev.from))}</b> knocked out <b>${esc(net.nameOf(ev.to))}</b>`);
      break;
    }
    case 'fire': {
      if (ev.id === me) break; // already shown when predicted
      const c = others.get(ev.id);
      c?.fire();
      const o = v3(ev.o), d = v3(ev.d);
      const from = c ? c.muzzle.clone() : o.clone();
      const s = spec(ev.w);
      const end = shotEnd(o, d);
      fx.tracer(from, end, s.muzzleVelocity);
      for (let i = 1; i < Math.min(s.pellets, 5); i++) {
        const jitter = d.clone().add(new THREE.Vector3().randomDirection().multiplyScalar(s.hipSpread * 0.8)).normalize();
        fx.tracer(from, shotEnd(o, jitter), s.muzzleVelocity);
      }
      fx.muzzleFlash(from, d, ev.w === 2 ? 0.4 : ev.w === 1 ? 0.22 : 0.3);
      const [pan] = spatial(from);
      sfx.gunshot(ev.w, pan, from.distanceTo(camera.position));
      // a round passing close to my head cracks
      const miss = closestApproach(o, end, camera.position);
      if (miss < 2.5) sfx.snap(spatial(end)[0], 1 - miss / 2.5);
      break;
    }
    case 'impact': {
      const p = v3(ev.p);
      fx.impact(p, v3(ev.n), ev.mat);
      const [pan, gain] = spatial(p, 4);
      sfx.impact(pan, gain * 0.7, ev.mat >= 4 && ev.mat <= 6);
      break;
    }
    case 'hit': {
      const p = v3(ev.p);
      const shooter = ev.from === me ? camera.position : positions.get(ev.from);
      fx.blood(p, shooter ? p.clone().sub(shooter).normalize() : new THREE.Vector3(0, 1, 0));
      if (ev.from === me) {
        flashHit(ev.zone === 'head' ? 'head' : '');
        sfx.hitMarker(ev.zone === 'head', false);
      }
      if (ev.to === me) {
        sfx.hurt();
        shake = Math.min(1, shake + ev.dmg / 60);
        if (shooter) hurtArc(shooter.clone());
      }
      break;
    }
    case 'kill': {
      const w = weapons()[ev.w]?.name ?? '';
      const tag = `<span class="w">${esc(w)}${ev.head ? ' · head' : ''}</span>`;
      const n = (id: number) => (id === me ? 'You' : `<b>${esc(net!.nameOf(id))}</b>`);
      feed(`${n(ev.from)} ${tag} ${ev.to === me ? 'you' : n(ev.to)}`);
      if (ev.from === me) { flashHit('kill'); sfx.hitMarker(ev.head, true); }
      if (ev.to === me) {
        deathNote = `Killed by <b>${esc(net.nameOf(ev.from))}</b><br>${esc(w)} · ${Math.round(ev.dist)} m${ev.head ? ' · headshot' : ''}`;
      }
      break;
    }
  }
}

/** Where a shot from `o` along `d` stops on the map (or 400 m out). */
function shotEnd(o: THREE.Vector3, d: THREE.Vector3) {
  return o.clone().addScaledVector(d, raycastBoxes(boxes, o, d, 400, 0));
}

function closestApproach(a: THREE.Vector3, b: THREE.Vector3, p: THREE.Vector3) {
  const ab = b.clone().sub(a);
  const t = THREE.MathUtils.clamp(p.clone().sub(a).dot(ab) / Math.max(1e-6, ab.lengthSq()), 0, 1);
  return a.clone().addScaledVector(ab, t).distanceTo(p);
}

function renderBoard() {
  if (!net) return;
  const slap = net.mode === 'slap';
  const head = slap ? '<th>KO</th><th>Slaps</th>' : '<th>K</th><th>D</th>';
  const rows = net.roster.map((r) => `<tr class="${r.id === net!.id ? 'me' : ''}"><td>${esc(r.name)}</td>` +
    (slap ? `<td>${r.kos}</td><td>${r.slaps}</td>` : `<td>${r.kills}</td><td>${r.deaths}</td>`) + '</tr>').join('');
  $('board').innerHTML = `<table><tr><th>Player</th>${head}</tr>${rows}</table>`;
}

function feed(html: string) {
  const d = document.createElement('div');
  d.innerHTML = html;
  $('feed').prepend(d);
  setTimeout(() => d.remove(), 6000);
  while ($('feed').children.length > 6) $('feed').lastElementChild!.remove();
}

function flashHit(kind: '' | 'head' | 'kill') {
  const h = $('hit');
  h.className = `on ${kind}`;
  requestAnimationFrame(() => requestAnimationFrame(() => h.classList.remove('on')));
}

function hurtArc(from: THREE.Vector3) {
  const el = document.createElement('div');
  el.className = 'arc';
  $('dmg').append(el);
  hurtFrom.push({ pos: from, age: 0, el });
  if (hurtFrom.length > 4) hurtFrom.shift()!.el.remove();
}

const positions = new Map<number, THREE.Vector3>();
/** Stereo pan and distance gain for a sound at a world position. */
function spatial(p: THREE.Vector3, ref = 6): [number, number] {
  const rel = p.clone().sub(camera.position);
  const d = rel.length();
  const right = new THREE.Vector3(1, 0, 0).applyQuaternion(camera.quaternion);
  return [d > 1e-3 ? rel.normalize().dot(right) : 0, Math.min(1, ref / Math.max(1, d))];
}
function spatialId(id: number | null): [number, number] {
  if (id == null) return [0, 1];
  const p = positions.get(id);
  return p ? spatial(p) : [0, 0.5];
}

// Debug handle for tooling and the console (read-only stats).
let fpsFrames = 0;
let fps = 0;
setInterval(() => { fps = fpsFrames; fpsFrames = 0; }, 1000);
Object.assign(window, { __riftline: { get fps() { return fps; }, renderer, scene, camera, input, get net() { return net; }, others, get self() { return selfChar; } } });

const DT = tickDt();
const camPos = new THREE.Vector3();
const pivot = new THREE.Vector3();
const fwd = new THREE.Vector3();
const rightV = new THREE.Vector3();
let acc = 0;
let last = performance.now();
let eye = eyeHeight(false);
let pivotY = 0;
let fov = 72;
let camDist = 2.8;
let prevReload = 0;
let prevMag = -1;
let prevWeapon = -1;
const xh = $('xh');

/** One fixed tick of local input: predict, send, and show my own shots immediately. */
function tick() {
  if (!net) return;
  const me = net.me;
  const flags = net.sendInput(input.sample({
    dt: DT,
    spec: me.armed ? spec(me.weapon) : null,
    ads: me.ads,
    crouched: me.crouched,
    speed: Math.hypot(me.vx, me.vz),
    stamina: me.stamina,
    firing: false,
  }));
  if (flags & Flag.Slap && net.mode === 'slap') {
    selfChar?.slap();
    sfx.whoosh(0, 0.8);
  }
  if (flags & Flag.Dry) sfx.dryFire();
  if (flags & Flag.Shot) {
    const s = me.shots();
    const w = me.weapon;
    const sp = spec(w);
    const muzzle = selfChar && camDist > 0.55 ? selfChar.muzzle.clone() : null;
    for (let i = 0; i < s.length; i += 6) {
      const o = new THREE.Vector3(s[i], s[i + 1], s[i + 2]);
      const d = new THREE.Vector3(s[i + 3], s[i + 4], s[i + 5]);
      if (i < 6 * 5) fx.tracer(muzzle ?? o.clone().addScaledVector(d, 1.5), shotEnd(o, d), sp.muzzleVelocity);
      if (i === 0) fx.muzzleFlash(muzzle ?? o.clone().addScaledVector(d, 0.6), d, w === 2 ? 0.4 : w === 1 ? 0.22 : 0.3);
    }
    sfx.gunshot(w, 0, 0);
    input.kick(me.kickPitch, me.kickYaw);
    selfChar?.fire();
  }
  // reload foley from my own predicted weapon state
  if (me.armed) {
    if (me.weapon !== prevWeapon) { prevMag = me.mag; prevWeapon = me.weapon; }
    if (prevReload <= 0 && me.reload > 0 && !spec(me.weapon).reloadPerShell) sfx.foley('out');
    if (me.mag > prevMag && prevMag >= 0) {
      if (spec(me.weapon).reloadPerShell) sfx.foley('shell');
      else {
        sfx.foley('in');
        if (prevMag === 0) setTimeout(() => sfx.foley('rack'), 250);
      }
    }
    if (prevMag > 0 && me.mag < prevMag && (me.weapon === 2 || me.weapon === 3)) setTimeout(() => sfx.foley('rack', 0, 0.5), 180);
    prevReload = me.reload;
    prevMag = me.mag;
  }
}

function frame(now: number) {
  fpsFrames++;
  const dt = Math.min(0.1, (now - last) / 1000);
  acc += Math.min(0.25, dt);
  last = now;

  if (net?.ready) {
    while (acc >= DT) { tick(); acc -= DT; }
  } else acc = 0;

  if (net?.ready && selfChar) {
    const me = net.me;
    const armed = me.armed;
    const dead = me.dead > 0;
    selfChar.update({ x: me.x, y: me.y, z: me.z, vx: me.vx, vz: me.vz, yaw: input.viewYaw, pitch: input.viewPitch,
      crouched: me.crouched, grounded: me.grounded, lean: me.lean, stun: me.stun, ko: me.ko,
      armed, weapon: me.weapon, ads: me.ads, reload: me.reload, reloadLen: me.reloadLen, swap: me.swap,
      lowered: me.lowered, dead: me.dead }, dt);

    const seen = new Set<number>();
    for (const p of net.remotes()) {
      seen.add(p.id);
      let c = others.get(p.id);
      if (!c) others.set(p.id, (c = new Character(scene, tints[p.id % tints.length])));
      c.update(p, dt);
      positions.set(p.id, new THREE.Vector3(p.x, p.y + 1.5, p.z));
    }
    for (const [id, c] of others) if (!seen.has(id)) { c.dispose(scene); others.delete(id); positions.delete(id); }

    // Over-the-shoulder camera. Its aim ray passes through the point shots leave
    // from (beside the head, SHOULDER metres right), so the crosshair is exact.
    const sp = armed ? spec(me.weapon) : null;
    const ads = armed ? me.ads : input.aiming ? 1 : 0;
    const scoped = !!sp && me.weapon === 3 && ads > 0.92 && !dead;
    eye += (eyeHeight(me.crouched) - eye) * Math.min(1, dt * 10);
    pivotY = Math.abs(me.y - pivotY) > 0.6 ? me.y : pivotY + (me.y - pivotY) * Math.min(1, dt * 14);
    const wantFov = sp ? 72 + (sp.adsFov - 72) * ads : aimFovSlap(ads);
    fov = scoped ? sp!.adsFov : fov + (wantFov - fov) * Math.min(1, dt * 14);
    camera.fov = fov;
    camera.updateProjectionMatrix();
    input.zoomScale = Math.tan((fov * Math.PI) / 360) / Math.tan((72 * Math.PI) / 360);
    const yaw = input.viewYaw, pitch = input.viewPitch;
    fwd.set(-Math.sin(yaw) * Math.cos(pitch), Math.sin(pitch), -Math.cos(yaw) * Math.cos(pitch));
    rightV.set(Math.cos(yaw), 0, -Math.sin(yaw));
    const down = me.ko > 0 || dead;
    pivot.set(me.x, pivotY + (down ? 0.5 : eye), me.z).addScaledVector(rightV, SHOULDER + me.lean * LEAN_SHIFT);
    const want = down ? 4.5 : scoped ? 0 : 2.8 + (1.25 - 2.8) * ads;
    const back = fwd.clone().negate();
    const hit = raycastBoxes(boxes, pivot, back, want, 0.2);
    camDist = hit < camDist ? hit : camDist + (hit - camDist) * Math.min(1, dt * 6);
    camPos.copy(pivot).addScaledVector(back, camDist);
    if (shake > 0) {
      camPos.add(new THREE.Vector3().randomDirection().multiplyScalar(shake * 0.04));
      shake = Math.max(0, shake - dt * 3);
    }
    camera.position.copy(camPos);
    camera.up.set(0, 1, 0);
    camera.lookAt(camPos.clone().add(fwd));
    camera.rotateZ(-me.lean * 0.06);
    selfChar.visible = camDist > 0.55;

    world.follow(me.x, me.z);

    // HUD
    const st = $('stamina');
    (st.firstElementChild as HTMLElement).style.transform = `scaleX(${me.stamina})`;
    st.classList.toggle('ex', me.exhausted);
    ($('daze').firstElementChild as HTMLElement).style.transform = `scaleX(${me.daze / 100})`;
    ($('hp').firstElementChild as HTMLElement).style.transform = `scaleX(${me.hp / 100})`;
    $('hp').classList.toggle('low', me.hp < 35);
    const hurt = armed ? Math.max(0, 1 - me.hp / 70) * 0.8 : me.daze / 100 + (me.stun > 0 ? 0.3 : 0);
    $('vignette').style.opacity = String(Math.min(1, hurt));
    $('ko').classList.toggle('hidden', !down);
    if (down) {
      $('ko').innerHTML = dead
        ? `<div>KILLED<small>${deathNote}</small><small>Respawning in ${Math.ceil(me.dead)}</small></div>`
        : '<div>KNOCKED OUT<small>Getting back up…</small></div>';
    } else deathNote = '';
    $('scope').classList.toggle('hidden', !scoped);
    if (sp) {
      $('mag').textContent = String(me.mag);
      $('mag').classList.toggle('low', me.mag <= Math.ceil(sp.mag / 4));
      $('res').textContent = `/ ${me.reserve}`;
      $('wname').textContent = `${sp.name}${sp.auto ? (me.semi ? ' · SEMI' : ' · AUTO') : ''}`;
      Array.from($("slots").children).forEach((c, i) => c.classList.toggle('on', i === me.weapon));
      const hint = me.reload > 0 ? 'Reloading…' : me.mag === 0 ? (me.reserve > 0 ? 'Press R to reload' : 'Out of ammo — switch weapon')
        : scoped ? (input.breath < 4 ? `Breath ${Math.round(input.breath * 25)}%` : 'Hold Shift to steady') : '';
      $('hint').textContent = hint;
      // crosshair gap shows the real shot cone
      const gap = (Math.tan(me.spread) / Math.tan((fov * Math.PI) / 360)) * (innerHeight / 2);
      xh.style.setProperty('--gap', `${Math.max(3, Math.min(120, gap))}px`);
      xh.classList.toggle('aimed', ads > 0.8 || dead);
    } else {
      xh.style.setProperty('--gap', '0px');
      xh.classList.add('aimed');
    }
    // damage arcs point at whoever hit me
    for (let i = hurtFrom.length - 1; i >= 0; i--) {
      const h = hurtFrom[i]!;
      h.age += dt;
      if (h.age > 2) { h.el.remove(); hurtFrom.splice(i, 1); continue; }
      const rel = h.pos.clone().sub(camera.position);
      const ang = Math.atan2(rel.dot(rightV), rel.dot(new THREE.Vector3(-Math.sin(yaw), 0, -Math.cos(yaw))));
      h.el.style.transform = `rotate(${ang}rad)`;
      h.el.style.opacity = String(1 - h.age / 2);
    }
  } else {
    // menu backdrop: slow orbit over the yard
    const t = now / 1000 * 0.05;
    camera.position.set(Math.sin(t) * 30, 14, Math.cos(t) * 30);
    camera.lookAt(0, 1.5, 0);
    world.follow(0, 0);
  }

  fx.update(dt);
  renderer.render(scene, camera);
  requestAnimationFrame(frame);
}

function aimFovSlap(ads: number) { return 72 - 24 * ads; }

requestAnimationFrame(frame);
