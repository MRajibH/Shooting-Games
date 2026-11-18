import * as THREE from 'three';
import init, { eyeHeight, leanShift, mapBoxes, mapHalf, shoulderOffset, tickDt } from './wasm/riftline_sim.js';
import { Sfx } from './audio';
import { Character, loadCharacter } from './character';
import { Fx } from './fx';
import { spec, weapons } from './guns';
import { Input } from './input';
import { Flag, Net } from './net';
import { Quality, loadSettings, saveSettings, type QualityLevel } from './quality';
import { TouchControls, isTouchDevice } from './touch';
import type { Difficulty, GameEvent, MatchState, Mode, RosterEntry, V3 } from './protocol';
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
const mobile = isTouchDevice();
document.body.classList.toggle('touch', mobile);
const settings = loadSettings();
const quality = new Quality(renderer, world.sun, settings.quality, mobile);
input.sensitivityScale = settings.sensitivity;
sfx.volume = settings.volume;
const touch = mobile ? new TouchControls(input) : null;
if (touch) {
  touch.visible = false;
  touch.onScore = () => document.body.classList.toggle('show-board');
}

// settings panel in the menu
const qualityEl = $<HTMLSelectElement>('quality');
const sensEl = $<HTMLInputElement>('sens');
const volEl = $<HTMLInputElement>('vol');
qualityEl.value = settings.quality;
sensEl.value = String(settings.sensitivity);
volEl.value = String(settings.volume);
const showSens = () => ($('sens-v').textContent = `${Number(sensEl.value).toFixed(2)}×`);
showSens();
qualityEl.addEventListener('change', () => {
  settings.quality = qualityEl.value as QualityLevel;
  quality.set(settings.quality, mobile);
  saveSettings(settings);
});
sensEl.addEventListener('input', () => {
  settings.sensitivity = Number(sensEl.value);
  input.sensitivityScale = settings.sensitivity;
  showSens();
  saveSettings(settings);
});
volEl.addEventListener('input', () => {
  settings.volume = Number(volEl.value);
  sfx.volume = settings.volume;
  saveSettings(settings);
});
const SHOULDER = shoulderOffset();
const LEAN_SHIFT = leanShift();

// Menu: pre-fill from the URL (?name=&room=&mode=) and last session.
const params = new URLSearchParams(location.search);
const nameEl = $<HTMLInputElement>('name');
const roomEl = $<HTMLInputElement>('room');
try { nameEl.value = params.get('name') ?? localStorage.getItem('riftline.name') ?? ''; } catch { /* storage blocked */ }
roomEl.value = params.get('room') ?? '';
const startMode = params.get('mode');
if (startMode === 'combat' || startMode === 'tdm' || startMode === 'domination' || startMode === 'slap') {
  ($<HTMLInputElement>('join').querySelector(`input[value=${startMode}]`) as HTMLInputElement).checked = true;
}
let savedSkill: string | null = null;
try { savedSkill = localStorage.getItem('riftline.ai'); } catch { /* storage blocked */ }
const startSkill = params.get('ai') ?? savedSkill;
if (startSkill === 'easy' || startSkill === 'normal' || startSkill === 'hard') {
  ($<HTMLInputElement>('join').querySelector(`input[name=ai][value=${startSkill}]`) as HTMLInputElement).checked = true;
}

let net: Net | null = null;
let selfChar: Character | null = null;
let selfTeam = -1;
const others = new Map<number, { c: Character; team: number }>();
const tints = [0xc9d6ff, 0xffd0b0, 0xc6ffcf, 0xffc4d8, 0xfff2b0, 0xd6c8ff, 0xb8f4ff, 0xffffff].map((c) => new THREE.Color(c));
/** Team 1 blue, team 2 red (index 0 unused). */
const TEAM_HEX = ['#ffffff', '#4f8fe8', '#e8584a'];
const TEAM_NAME = ['', 'Blue', 'Red'];
const TEAM_COL = TEAM_HEX.map((h) => new THREE.Color(h));
const newChar = (id: number, team: number) =>
  team ? new Character(scene, TEAM_COL[team], 0.6) : new Character(scene, tints[id % tints.length]);

$('join').addEventListener('submit', async (e) => {
  e.preventDefault();
  sfx.unlock();
  const name = nameEl.value.trim();
  try { localStorage.setItem('riftline.name', name); } catch { /* storage blocked */ }
  const form = new FormData(e.target as HTMLFormElement);
  const mode = (form.get('mode') ?? 'combat') as Mode;
  const ai = (form.get('ai') ?? 'normal') as Difficulty;
  try { localStorage.setItem('riftline.ai', ai); } catch { /* storage blocked */ }
  await charLoad;
  $('menu').classList.add('hidden');
  $('hud').classList.remove('hidden');
  document.body.classList.add('playing');
  if (mobile) {
    // phones: go full screen and landscape where the browser allows it
    await document.documentElement.requestFullscreen?.().catch(() => {});
    await (screen.orientation as ScreenOrientation & { lock?: (o: string) => Promise<void> }).lock?.('landscape').catch(() => {});
    touch!.visible = true;
    touch!.setArmed(mode !== 'slap');
  } else renderer.domElement.requestPointerLock()?.catch?.(() => {});
  start(name, roomEl.value.trim() || undefined, mode, ai);
});

const esc = (s: string) => s.replace(/[&<>"]/g, (c) => `&#${c.charCodeAt(0)};`);
/** A player's name, coloured by team. */
const who = (id: number) => {
  const t = net?.teamOf(id) ?? 0;
  return `<b style="color:${t ? TEAM_HEX[t] : 'inherit'}">${esc(net?.nameOf(id) ?? '')}</b>`;
};
const v3 = (a: V3) => new THREE.Vector3(a[0], a[1], a[2]);

/** Who killed me, for the death screen. */
let deathNote = '';
/** Last hits on me: where they came from (for the damage direction arcs). */
const hurtFrom: { pos: THREE.Vector3; age: number; el: HTMLElement }[] = [];
let shake = 0;

function start(name: string, room: string | undefined, mode: Mode, ai: Difficulty) {
  net = new Net(name, room, mode, ai);
  net.onStatus = (s) => ($('status').textContent = s);
  net.onWelcome = () => {
    const combat = net!.mode !== 'slap';
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
      if (ev.from !== me) others.get(ev.from)?.c.slap();
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
      const c = others.get(ev.id)?.c;
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
    case 'capture': {
      const letter = 'ABC'[ev.flag] ?? '?';
      const mine = ev.team === net.me.team;
      feed(`<b style="color:${TEAM_HEX[ev.team]}">${TEAM_NAME[ev.team]}</b> took <b>${letter}</b>`);
      sfx.hitMarker(!mine, mine);
      break;
    }
    case 'kill': {
      const w = weapons()[ev.w]?.name ?? '';
      const tag = `<span class="w">${esc(w)}${ev.head ? ' · head' : ''}</span>`;
      const n = (id: number) => (id === me ? 'You' : who(id));
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
  $('board').innerHTML = boardTable();
  if (net.game?.phase === 'ended') renderResults(net.game);
}

/** Scoreboard rows, grouped by team in team modes. */
function boardTable() {
  if (!net) return '';
  const mode = net.mode;
  const cols: [string, (r: RosterEntry) => number][] =
    mode === 'slap' ? [['KO', (r) => r.kos], ['Slaps', (r) => r.slaps]]
      : mode === 'domination' ? [['K', (r) => r.kills], ['D', (r) => r.deaths], ['Caps', (r) => r.caps]]
        : [['K', (r) => r.kills], ['D', (r) => r.deaths], ['Score', (r) => r.score]];
  const row = (r: RosterEntry) => `<tr class="${r.id === net!.id ? 'me' : ''}">` +
    `<td>${r.team ? `<i class="dot" style="background:${TEAM_HEX[r.team]}"></i>` : ''}${esc(r.name)}${r.bot ? ' <span class="bot">BOT</span>' : ''}</td>` +
    cols.map(([, f]) => `<td>${f(r)}</td>`).join('') + '</tr>';
  return `<table><tr><th>Player</th>${cols.map(([h]) => `<th>${h}</th>`).join('')}</tr>${net.roster.map(row).join('')}</table>`;
}

let resultsShown = false;
function renderResults(g: MatchState) {
  if (!net) return;
  const myTeam = net.me.team;
  let title: string;
  if (net.mode === 'tdm' || net.mode === 'domination') {
    title = g.winnerTeam === 0 ? 'DRAW' : g.winnerTeam === myTeam ? 'VICTORY' : 'DEFEAT';
  } else {
    title = g.winnerId === net.id ? 'YOU WIN' : g.winnerId ? `${esc(net.nameOf(g.winnerId))} WINS` : 'DRAW';
  }
  const scores = net.mode === 'tdm' || net.mode === 'domination'
    ? `<div class="big"><span style="color:${TEAM_HEX[1]}">${g.scores[0]}</span> : <span style="color:${TEAM_HEX[2]}">${g.scores[1]}</span></div>` : '';
  $('results').innerHTML = `<div class="card"><h2>${title}</h2>${scores}` +
    (g.mvp ? `<p>MVP <b>${esc(net.nameOf(g.mvp))}</b></p>` : '') + boardTable() +
    `<p class="next">Next match in <span id="next-in">${Math.ceil(g.nextIn)}</span></p></div>`;
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
Object.assign(window, { __riftline: { get fps() { return fps; }, renderer, scene, camera, input, quality, touch, get net() { return net; }, others, get self() { return selfChar; } } });

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
    if (touch) aimAssist(dt);
    while (acc >= DT) { tick(); acc -= DT; }
  } else acc = 0;

  // my character appears once the server has told me my team (and changes with it)
  if (net?.ready && net.game && selfTeam !== net.me.team) {
    selfChar?.dispose(scene);
    selfChar = newChar(net.id, net.me.team);
    selfTeam = net.me.team;
  }

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
      let o = others.get(p.id);
      if (!o || o.team !== p.team) {
        o?.c.dispose(scene);
        o = { c: newChar(p.id, p.team), team: p.team };
        others.set(p.id, o);
      }
      o.c.update(p, dt);
      positions.set(p.id, new THREE.Vector3(p.x, p.y + 1.5, p.z));
    }
    for (const [id, o] of others) if (!seen.has(id)) { o.c.dispose(scene); others.delete(id); positions.delete(id); }

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
    if (net.game) matchHud(net.game, now / 1000);
    nameTags(me.team);
  } else {
    // menu backdrop: slow orbit over the yard
    const t = now / 1000 * 0.05;
    camera.position.set(Math.sin(t) * 30, 14, Math.cos(t) * 30);
    camera.lookAt(0, 1.5, 0);
    world.follow(0, 0);
  }

  fx.update(dt);
  quality.frame();
  renderer.render(scene, camera);
  requestAnimationFrame(frame);
}

function aimFovSlap(ads: number) { return 72 - 24 * ads; }

// ---- match HUD, capture points, team name tags ----

const FLAG_RADIUS = 4;
const flagObjs: { cloth: THREE.Mesh; ring: THREE.Mesh; fill: THREE.Mesh; el: HTMLElement; x: number; z: number }[] = [];
const fmtTime = (t: number) => `${Math.floor(t / 60)}:${String(Math.floor(t % 60)).padStart(2, '0')}`;
const project = (p: THREE.Vector3) => {
  const v = p.clone().project(camera);
  return v.z < 1 && Math.abs(v.x) < 1.2 && Math.abs(v.y) < 1.2 ? [(v.x + 1) / 2 * innerWidth, (1 - v.y) / 2 * innerHeight] : null;
};

function buildFlags(g: MatchState) {
  const steel = new THREE.MeshStandardMaterial({ color: 0x8a8f94, roughness: 0.4, metalness: 0.8 });
  g.flags.forEach((f, i) => {
    const pole = new THREE.Mesh(new THREE.CylinderGeometry(0.035, 0.045, 3.4, 10), steel);
    pole.position.set(f.x, 1.7, f.z);
    pole.castShadow = true;
    const cloth = new THREE.Mesh(new THREE.PlaneGeometry(1.1, 0.7, 8, 4),
      new THREE.MeshStandardMaterial({ color: 0xffffff, roughness: 0.9, side: THREE.DoubleSide }));
    cloth.geometry.translate(0.55, 0, 0);
    cloth.position.set(f.x + 0.04, 3.0, f.z);
    cloth.castShadow = true;
    const ring = new THREE.Mesh(new THREE.RingGeometry(FLAG_RADIUS - 0.12, FLAG_RADIUS, 64),
      new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.55, depthWrite: false }));
    ring.rotation.x = -Math.PI / 2;
    ring.position.set(f.x, 0.03, f.z);
    const fill = new THREE.Mesh(new THREE.CircleGeometry(FLAG_RADIUS - 0.12, 64),
      new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.08, depthWrite: false }));
    fill.rotation.x = -Math.PI / 2;
    fill.position.set(f.x, 0.025, f.z);
    scene.add(pole, cloth, ring, fill);
    const el = document.createElement('div');
    el.className = 'flagtag';
    el.textContent = 'ABC'[i]!;
    $('tags').append(el);
    flagObjs.push({ cloth, ring, fill, el, x: f.x, z: f.z });
  });
}

function matchHud(g: MatchState, t: number) {
  if (!net) return;
  if (!flagObjs.length && g.flags.length) buildFlags(g);
  const teams = net.mode === 'tdm' || net.mode === 'domination';
  const myTeam = net.me.team;
  let html: string;
  if (teams) {
    const side = (k: 1 | 2) => `<span class="ts${myTeam === k ? ' mine' : ''}" style="color:${TEAM_HEX[k]}">${g.scores[k - 1]}</span>`;
    html = `${side(1)}<span class="clock">${fmtTime(g.timeLeft)}<small>first to ${g.limit}</small></span>${side(2)}`;
    if (g.flags.length) {
      html += '<div class="flags">' + g.flags.map((f, i) => {
        const col = f.owner ? TEAM_HEX[f.owner] : 'rgba(255,255,255,.15)';
        const pc = Math.abs(f.progress) * 100;
        const toward = f.progress < 0 ? TEAM_HEX[1] : TEAM_HEX[2];
        return `<span style="border-color:${col}"><i style="width:${pc}%;background:${toward}"></i><b>${'ABC'[i]}</b></span>`;
      }).join('') + '</div>';
    }
  } else {
    const val = (r: RosterEntry) => (net!.mode === 'slap' ? r.kos : r.kills);
    const mine = net.roster.find((r) => r.id === net!.id);
    const lead = net.roster.filter((r) => r.id !== net!.id).reduce((m, r) => Math.max(m, val(r)), 0);
    html = `<span class="ts mine">${mine ? val(mine) : 0}</span><span class="clock">${fmtTime(g.timeLeft)}<small>first to ${g.limit}</small></span><span class="ts" title="leader">${lead}</span>`;
  }
  const bar = $('matchbar');
  if (bar.dataset.html !== html) { bar.innerHTML = html; bar.dataset.html = html; }

  // capture points in the world and on screen
  flagObjs.forEach((o, i) => {
    const f = g.flags[i];
    if (!f) return;
    const owner = f.owner ? TEAM_COL[f.owner]! : new THREE.Color(0xdddddd);
    (o.cloth.material as THREE.MeshStandardMaterial).color.copy(owner);
    (o.ring.material as THREE.MeshBasicMaterial).color.copy(f.progress < 0 ? TEAM_COL[1]! : f.progress > 0 ? TEAM_COL[2]! : owner);
    (o.fill.material as THREE.MeshBasicMaterial).color.copy(owner);
    o.cloth.rotation.y = Math.sin(t * 1.3 + i) * 0.25;
    const pos = project(new THREE.Vector3(o.x, 3.9, o.z));
    o.el.style.display = pos ? '' : 'none';
    if (pos) {
      o.el.style.transform = `translate(${pos[0]}px, ${pos[1]}px)`;
      o.el.style.borderColor = f.owner ? TEAM_HEX[f.owner]! : '#ddd';
      o.el.dataset.d = `${Math.round(Math.hypot(o.x - net!.me.x, o.z - net!.me.z))} m`;
    }
  });

  // results screen
  const ended = g.phase === 'ended';
  $('results').classList.toggle('hidden', !ended);
  if (ended && !resultsShown) renderResults(g);
  resultsShown = ended;
  const n = document.getElementById('next-in');
  if (ended && n) n.textContent = String(Math.ceil(g.nextIn));
}

/** Teammates' names over their heads (enemies get none, like real life). */
const tagEls = new Map<number, HTMLElement>();
function nameTags(myTeam: number) {
  if (!net) return;
  const live = new Set<number>();
  if (myTeam) {
    for (const p of net.remotes()) {
      if (p.team !== myTeam || p.dead > 0) continue;
      const pos = project(new THREE.Vector3(p.x, p.y + (p.crouched ? 1.55 : 2.1), p.z));
      if (!pos) continue;
      live.add(p.id);
      let el = tagEls.get(p.id);
      if (!el) {
        el = document.createElement('div');
        el.className = 'nametag';
        $('tags').append(el);
        tagEls.set(p.id, el);
      }
      el.textContent = net.nameOf(p.id);
      el.style.color = TEAM_HEX[myTeam]!;
      el.style.transform = `translate(${pos[0]}px, ${pos[1]}px)`;
    }
  }
  for (const [id, el] of tagEls) if (!live.has(id)) { el.remove(); tagEls.delete(id); }
}

requestAnimationFrame(frame);

/**
 * Touch aim assist: when an enemy's chest is close to the crosshair and in plain
 * sight, looking slows down over them and the aim drifts gently onto them.
 * Mouse players get none.
 */
function aimAssist(dt: number) {
  input.friction = 1;
  if (!net || !net.me.armed || net.me.dead > 0) return;
  const cone = net.me.ads > 0.5 ? 0.06 : 0.1;
  const f = new THREE.Vector3(-Math.sin(input.yaw) * Math.cos(input.pitch), Math.sin(input.pitch), -Math.cos(input.yaw) * Math.cos(input.pitch));
  const eye = camera.position;
  let best: { dir: THREE.Vector3; angle: number } | null = null;
  for (const p of net.remotes()) {
    if (p.dead > 0 || (net.me.team && p.team === net.me.team)) continue;
    const d = new THREE.Vector3(p.x, p.y + (p.crouched ? 0.8 : 1.2), p.z).sub(eye);
    const dist = d.length();
    if (dist > 70 || dist < 0.5) continue;
    d.divideScalar(dist);
    const angle = Math.acos(Math.min(1, d.dot(f)));
    if (angle > cone || (best && angle > best.angle)) continue;
    if (raycastBoxes(boxes, eye, d, dist, 0) < dist - 0.3) continue;
    best = { dir: d, angle };
  }
  if (!best) return;
  input.friction = 0.5;
  const ty = Math.atan2(-best.dir.x, -best.dir.z);
  const tp = Math.asin(best.dir.y);
  const pull = 0.5 * dt * (1 - best.angle / cone);
  const dy = Math.atan2(Math.sin(ty - input.yaw), Math.cos(ty - input.yaw));
  input.yaw += Math.max(-pull, Math.min(pull, dy));
  input.pitch += Math.max(-pull, Math.min(pull, tp - input.pitch));
}
