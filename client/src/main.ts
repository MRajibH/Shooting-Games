import * as THREE from 'three';
import init, { eyeHeight, mapBoxes, mapHalf, tickDt } from './wasm/riftline_sim.js';
import { Sfx } from './audio';
import { Character, loadCharacter } from './character';
import { Input } from './input';
import { Net } from './net';
import type { Mode } from './protocol';
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
const input = new Input(renderer.domElement);

// Menu: pre-fill from the URL (?name=&room=&mode=) and last session.
const params = new URLSearchParams(location.search);
const nameEl = $<HTMLInputElement>('name');
const roomEl = $<HTMLInputElement>('room');
try { nameEl.value = params.get('name') ?? localStorage.getItem('riftline.name') ?? ''; } catch { /* storage blocked */ }
roomEl.value = params.get('room') ?? '';
if (params.get('mode') === 'combat') ($<HTMLInputElement>('join').querySelector('input[value=combat]') as HTMLInputElement).checked = true;

let net: Net | null = null;
let selfChar: Character | null = null;
const others = new Map<number, Character>();
const tints = [0xc9d6ff, 0xffd0b0, 0xc6ffcf, 0xffc4d8, 0xfff2b0, 0xd6c8ff, 0xb8f4ff, 0xffffff].map((c) => new THREE.Color(c));

$('join').addEventListener('submit', async (e) => {
  e.preventDefault();
  sfx.unlock();
  const name = nameEl.value.trim();
  try { localStorage.setItem('riftline.name', name); } catch { /* storage blocked */ }
  const mode = (new FormData(e.target as HTMLFormElement).get('mode') ?? 'slap') as Mode;
  await charLoad;
  $('menu').classList.add('hidden');
  $('hud').classList.remove('hidden');
  renderer.domElement.requestPointerLock()?.catch?.(() => {});
  start(name, roomEl.value.trim() || undefined, mode);
});

function start(name: string, room: string | undefined, mode: Mode) {
  net = new Net(name, room, mode);
  net.onStatus = (s) => ($('status').textContent = s);
  net.onWelcome = () => {
    selfChar = new Character(scene, tints[net!.id % tints.length]);
    $('daze-wrap').classList.toggle('hidden', net!.mode !== 'slap');
    $('crosshair').classList.toggle('hidden', false);
  };
  net.onRoster = renderBoard;
  net.onEvent = (ev) => {
    if (!net) return;
    if (ev.t === 'slap') {
      if (ev.from !== net.id) others.get(ev.from)?.slap();
      if (ev.to != null) {
        const [pan, gain] = spatial(ev.to === net.id ? null : ev.to);
        sfx.slapHit(pan, gain);
        if (ev.from === net.id) flashHit();
      } else if (ev.from !== net.id) {
        const [pan, gain] = spatial(ev.from);
        sfx.whoosh(pan, gain);
      }
    } else if (ev.t === 'ko') {
      const [pan, gain] = spatial(ev.to === net.id ? null : ev.to);
      sfx.knockout(pan, gain);
      feed(ev.from === net.id ? `You knocked out <b>${esc(net.nameOf(ev.to))}</b>` :
        ev.to === net.id ? `<b>${esc(net.nameOf(ev.from))}</b> knocked you out` :
        `<b>${esc(net.nameOf(ev.from))}</b> knocked out <b>${esc(net.nameOf(ev.to))}</b>`);
    }
  };
}

const esc = (s: string) => s.replace(/[&<>"]/g, (c) => `&#${c.charCodeAt(0)};`);

function renderBoard() {
  if (!net) return;
  const rows = net.roster.map((r) => `<tr class="${r.id === net!.id ? 'me' : ''}"><td>${esc(r.name)}</td>` +
    (net!.mode === 'slap' ? `<td>${r.kos}</td><td>${r.slaps}</td>` : '') + '</tr>').join('');
  $('board').innerHTML = `<table><tr><th>Player</th>${net.mode === 'slap' ? '<th>KO</th><th>Slaps</th>' : ''}</tr>${rows}</table>`;
}

function feed(html: string) {
  const d = document.createElement('div');
  d.innerHTML = html;
  $('feed').prepend(d);
  setTimeout(() => d.remove(), 5000);
  while ($('feed').children.length > 5) $('feed').lastElementChild!.remove();
}

function flashHit() {
  const h = $('hit');
  h.classList.add('on');
  requestAnimationFrame(() => requestAnimationFrame(() => h.classList.remove('on')));
}

const positions = new Map<number, THREE.Vector3>();
/** Stereo pan and distance gain for a sound at a player's position (null = at me). */
function spatial(id: number | null): [number, number] {
  if (id == null) return [0, 1];
  const p = positions.get(id);
  if (!p) return [0, 0.5];
  const rel = p.clone().sub(camera.position);
  const d = rel.length();
  const right = new THREE.Vector3(1, 0, 0).applyQuaternion(camera.quaternion);
  return [rel.normalize().dot(right), Math.min(1, 6 / Math.max(1, d))];
}

// Debug handle for tooling and the console (read-only stats).
let fpsFrames = 0;
let fps = 0;
setInterval(() => { fps = fpsFrames; fpsFrames = 0; }, 1000);
Object.assign(window, { __riftline: { get fps() { return fps; }, renderer, scene, camera, get net() { return net; }, others } });

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

function frame(now: number) {
  fpsFrames++;
  const dt = Math.min(0.1, (now - last) / 1000);
  acc += Math.min(0.25, dt);
  last = now;

  if (net?.ready) {
    while (acc >= DT) {
      if (net.sendInput(input.sample()) && net.mode === 'slap') {
        selfChar?.slap();
        sfx.whoosh(0, 0.8);
      }
      acc -= DT;
    }
  } else acc = 0;

  if (net?.ready && selfChar) {
    const me = net.me;
    selfChar.update({ x: me.x, y: me.y, z: me.z, vx: me.vx, vz: me.vz, yaw: input.yaw, pitch: input.pitch,
      crouched: me.crouched, grounded: me.grounded, lean: me.lean, stun: me.stun, ko: me.ko }, dt);

    const seen = new Set<number>();
    for (const p of net.remotes()) {
      seen.add(p.id);
      let c = others.get(p.id);
      if (!c) others.set(p.id, (c = new Character(scene, tints[p.id % tints.length])));
      c.update(p, dt);
      positions.set(p.id, new THREE.Vector3(p.x, p.y + 1.5, p.z));
    }
    for (const [id, c] of others) if (!seen.has(id)) { c.dispose(scene); others.delete(id); positions.delete(id); }

    // Over-the-shoulder camera with lean offset, aim zoom and wall collision.
    const aim = input.aiming;
    eye += (eyeHeight(me.crouched) - eye) * Math.min(1, dt * 10);
    pivotY = Math.abs(me.y - pivotY) > 0.6 ? me.y : pivotY + (me.y - pivotY) * Math.min(1, dt * 14);
    fov += ((aim ? 48 : 72) - fov) * Math.min(1, dt * 12);
    camera.fov = fov;
    camera.updateProjectionMatrix();
    const yaw = input.yaw, pitch = input.pitch;
    fwd.set(-Math.sin(yaw) * Math.cos(pitch), Math.sin(pitch), -Math.cos(yaw) * Math.cos(pitch));
    rightV.set(Math.cos(yaw), 0, -Math.sin(yaw));
    const ko = me.ko > 0;
    pivot.set(me.x, pivotY + (ko ? 0.5 : eye), me.z).addScaledVector(rightV, (aim ? 0.42 : 0.55) + me.lean * 0.45);
    const want = ko ? 4.5 : aim ? 1.3 : 2.8;
    const back = fwd.clone().negate();
    const hit = raycastBoxes(boxes, pivot, back, want, 0.2);
    camDist = hit < camDist ? hit : camDist + (hit - camDist) * Math.min(1, dt * 6);
    camPos.copy(pivot).addScaledVector(back, camDist);
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
    $('vignette').style.opacity = String(Math.min(1, me.daze / 100 + (me.stun > 0 ? 0.3 : 0)));
    $('ko').classList.toggle('hidden', !ko);
  } else {
    // menu backdrop: slow orbit over the yard
    const t = now / 1000 * 0.05;
    camera.position.set(Math.sin(t) * 30, 14, Math.cos(t) * 30);
    camera.lookAt(0, 1.5, 0);
    world.follow(0, 0);
  }

  renderer.render(scene, camera);
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
