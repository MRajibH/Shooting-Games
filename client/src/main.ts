import * as THREE from 'three';
import { TICK_DT } from '@riftline/shared';
import { Input } from './input';
import { Net } from './net';

const status = document.getElementById('status')!;
const renderer = new THREE.WebGLRenderer({ antialias: true });
renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
document.body.appendChild(renderer.domElement);

const scene = new THREE.Scene();
scene.background = new THREE.Color(0xdfe9f2);
scene.add(new THREE.HemisphereLight(0xffffff, 0x8899aa, 1.2));
const sun = new THREE.DirectionalLight(0xffffff, 1.5);
sun.position.set(20, 40, 10);
scene.add(sun);
const ground = new THREE.Mesh(
  new THREE.PlaneGeometry(120, 120),
  new THREE.MeshStandardMaterial({ color: 0xb8c4cf }),
);
ground.rotation.x = -Math.PI / 2;
scene.add(ground, new THREE.GridHelper(120, 60, 0x66788a, 0x9aa8b5));

const camera = new THREE.PerspectiveCamera(70, 1, 0.1, 500);
const resize = () => {
  renderer.setSize(innerWidth, innerHeight);
  camera.aspect = innerWidth / innerHeight;
  camera.updateProjectionMatrix();
};
addEventListener('resize', resize);
resize();

// Placeholder avatars until the character rig lands.
const makeAvatar = (color: number) => {
  const m = new THREE.Mesh(new THREE.BoxGeometry(0.8, 1.8, 0.8), new THREE.MeshStandardMaterial({ color }));
  m.geometry.translate(0, 0.9, 0);
  scene.add(m);
  return m;
};
const self = makeAvatar(0xff7a1a);
const others = new Map<number, THREE.Mesh>();

const input = new Input(renderer.domElement);
const params = new URLSearchParams(location.search);
const net = new Net(params.get('name') ?? '', params.get('room') ?? undefined);
net.onStatus = (s) => (status.textContent = s);

let acc = 0;
let last = performance.now();
function frame(now: number) {
  acc += Math.min(0.25, (now - last) / 1000);
  last = now;
  while (acc >= TICK_DT) {
    net.sendInput(input.sample());
    acc -= TICK_DT;
  }

  self.position.set(net.me.x, net.me.y, net.me.z);
  self.rotation.y = input.yaw;

  const seen = new Set<number>();
  for (const p of net.remotes()) {
    seen.add(p.id);
    let m = others.get(p.id);
    if (!m) others.set(p.id, (m = makeAvatar(0x2a7fff)));
    m.position.set(p.x, p.y, p.z);
    m.rotation.y = p.yaw;
  }
  for (const [id, m] of others) if (!seen.has(id)) { scene.remove(m); others.delete(id); }

  // third-person camera behind and above the player
  const dist = 6;
  const cp = Math.cos(input.pitch);
  camera.position.set(
    net.me.x + Math.sin(input.yaw) * cp * dist,
    net.me.y + 2 - Math.sin(input.pitch) * dist,
    net.me.z + Math.cos(input.yaw) * cp * dist,
  );
  camera.position.y = Math.max(0.3, camera.position.y);
  camera.lookAt(net.me.x, net.me.y + 1.5, net.me.z);

  renderer.render(scene, camera);
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
