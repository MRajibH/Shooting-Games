import * as THREE from 'three';
import { Sky } from 'three/examples/jsm/objects/Sky.js';
import init, { eyeHeight, tickDt } from './wasm/riftline_sim.js';
import { Input } from './input';
import { Net } from './net';

await init();

const status = document.getElementById('status')!;
const renderer = new THREE.WebGLRenderer({ antialias: true, powerPreference: 'high-performance' });
renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
// Physically based baseline: filmic tone mapping, soft shadows, real sun and sky.
renderer.toneMapping = THREE.ACESFilmicToneMapping;
renderer.toneMappingExposure = 0.6;
renderer.shadowMap.enabled = true;
renderer.shadowMap.type = THREE.PCFSoftShadowMap;
document.body.appendChild(renderer.domElement);

const scene = new THREE.Scene();
scene.fog = new THREE.FogExp2(0xc9d3dc, 0.006);

const sunDir = new THREE.Vector3().setFromSphericalCoords(1, THREE.MathUtils.degToRad(55), THREE.MathUtils.degToRad(140));
const sky = new Sky();
sky.scale.setScalar(4000);
Object.assign(sky.material.uniforms, {});
sky.material.uniforms.turbidity!.value = 6;
sky.material.uniforms.rayleigh!.value = 1.6;
sky.material.uniforms.sunPosition!.value.copy(sunDir);
scene.add(sky);
const pmrem = new THREE.PMREMGenerator(renderer);
const envScene = new THREE.Scene();
envScene.add(sky.clone());
scene.environment = pmrem.fromScene(envScene).texture;

const sun = new THREE.DirectionalLight(0xfff1dd, 3.2);
sun.position.copy(sunDir).multiplyScalar(80);
sun.castShadow = true;
sun.shadow.mapSize.set(2048, 2048);
sun.shadow.bias = -0.0004;
Object.assign(sun.shadow.camera, { left: -40, right: 40, top: 40, bottom: -40, far: 200 });
scene.add(sun, sun.target);

const ground = new THREE.Mesh(
  new THREE.PlaneGeometry(400, 400),
  new THREE.MeshStandardMaterial({ color: 0x7d7a70, roughness: 0.95 }),
);
ground.rotation.x = -Math.PI / 2;
ground.receiveShadow = true;
scene.add(ground);

// Scale references (1 m crates) so speeds read correctly until the map lands.
const crateMat = new THREE.MeshStandardMaterial({ color: 0x8a6a45, roughness: 0.8 });
for (let i = 0; i < 24; i++) {
  const c = new THREE.Mesh(new THREE.BoxGeometry(1, 1, 1), crateMat);
  c.position.set(Math.cos(i * 2.4) * (6 + i * 1.3), 0.5, Math.sin(i * 2.4) * (6 + i * 1.3));
  c.castShadow = c.receiveShadow = true;
  scene.add(c);
}

const camera = new THREE.PerspectiveCamera(75, 1, 0.05, 3000);
const resize = () => {
  renderer.setSize(innerWidth, innerHeight);
  camera.aspect = innerWidth / innerHeight;
  camera.updateProjectionMatrix();
};
addEventListener('resize', resize);
resize();

// Placeholder bodies (1.8 m capsules) until the character rig lands.
const makeBody = (color: number) => {
  const g = new THREE.CapsuleGeometry(0.28, 1.24, 4, 12);
  g.translate(0, 0.9, 0);
  const m = new THREE.Mesh(g, new THREE.MeshStandardMaterial({ color, roughness: 0.7 }));
  m.castShadow = true;
  scene.add(m);
  return m;
};
const self = makeBody(0x4f5d3a);
const others = new Map<number, THREE.Mesh>();

const input = new Input(renderer.domElement);
const params = new URLSearchParams(location.search);
const net = new Net(params.get('name') ?? '', params.get('room') ?? undefined);
net.onStatus = (s) => (status.textContent = s);

const DT = tickDt();
const camPos = new THREE.Vector3();
const fwd = new THREE.Vector3();
let acc = 0;
let last = performance.now();
let eye = eyeHeight(false);
let fov = 75;

function frame(now: number) {
  const dt = Math.min(0.25, (now - last) / 1000);
  acc += dt;
  last = now;
  while (acc >= DT) {
    net.sendInput(input.sample());
    acc -= DT;
  }

  const me = net.me;
  self.position.set(me.x, me.y, me.z);
  self.rotation.y = input.yaw;
  self.scale.y = me.crouched ? 0.65 : 1;

  const seen = new Set<number>();
  for (const p of net.remotes()) {
    seen.add(p.id);
    let m = others.get(p.id);
    if (!m) others.set(p.id, (m = makeBody(0x6b4a3a)));
    m.position.set(p.x, p.y, p.z);
    m.rotation.y = p.yaw;
    m.scale.y = p.crouched ? 0.65 : 1;
  }
  for (const [id, m] of others) if (!seen.has(id)) { scene.remove(m); others.delete(id); }

  // Over-the-shoulder camera: smooth crouch height, tighter and narrower when aiming.
  eye += (eyeHeight(me.crouched) - eye) * Math.min(1, dt * 10);
  const aim = input.aiming;
  fov += ((aim ? 50 : 75) - fov) * Math.min(1, dt * 12);
  camera.fov = fov;
  camera.updateProjectionMatrix();
  const dist = aim ? 1.4 : 2.8;
  const shoulder = aim ? 0.45 : 0.6;
  fwd.set(-Math.sin(input.yaw) * Math.cos(input.pitch), Math.sin(input.pitch), -Math.cos(input.yaw) * Math.cos(input.pitch));
  camPos.set(me.x, me.y + eye, me.z)
    .addScaledVector(fwd, -dist)
    .add(new THREE.Vector3(Math.cos(input.yaw), 0, -Math.sin(input.yaw)).multiplyScalar(shoulder));
  camPos.y = Math.max(0.2, camPos.y);
  camera.position.copy(camPos);
  camera.lookAt(camPos.clone().add(fwd));

  sun.target.position.set(me.x, 0, me.z);
  sun.position.copy(sunDir).multiplyScalar(80).add(sun.target.position);

  renderer.render(scene, camera);
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);
