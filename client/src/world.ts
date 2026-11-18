import * as THREE from 'three';
import { Sky } from 'three/examples/jsm/objects/Sky.js';
import { mergeGeometries } from 'three/examples/jsm/utils/BufferGeometryUtils.js';
import { groundMaterial, mapMaterials } from './textures';

export interface Box { min: THREE.Vector3; max: THREE.Vector3; mat: number }

export function parseBoxes(f: Float32Array): Box[] {
  const out: Box[] = [];
  for (let i = 0; i + 6 < f.length; i += 7) {
    out.push({ min: new THREE.Vector3(f[i], f[i + 1], f[i + 2]), max: new THREE.Vector3(f[i + 3], f[i + 4], f[i + 5]), mat: f[i + 6]! });
  }
  return out;
}

/** Box geometry with world-space UVs so textures keep real-world scale on any size. */
function boxGeometry(b: Box, tile: number) {
  const size = new THREE.Vector3().subVectors(b.max, b.min);
  const center = new THREE.Vector3().addVectors(b.min, b.max).multiplyScalar(0.5);
  const g = new THREE.BoxGeometry(size.x, size.y, size.z);
  g.translate(center.x, center.y, center.z);
  const pos = g.getAttribute('position');
  const uv = g.getAttribute('uv');
  for (let i = 0; i < pos.count; i++) {
    const face = Math.floor(i / 4); // +x -x +y -y +z -z
    const x = pos.getX(i), y = pos.getY(i), z = pos.getZ(i);
    const [u, v] = face < 2 ? [z, y] : face < 4 ? [x, z] : [x, y];
    uv.setXY(i, u / tile, v / tile);
  }
  return g;
}

export function buildWorld(renderer: THREE.WebGLRenderer, scene: THREE.Scene, boxes: Box[], half: number) {
  // sky, sun and image-based lighting from the same sky
  const sunDir = new THREE.Vector3().setFromSphericalCoords(1, THREE.MathUtils.degToRad(52), THREE.MathUtils.degToRad(145));
  const sky = new Sky();
  sky.scale.setScalar(4000);
  const u = sky.material.uniforms;
  u.turbidity!.value = 5;
  u.rayleigh!.value = 1.4;
  u.mieCoefficient!.value = 0.004;
  u.mieDirectionalG!.value = 0.85;
  u.sunPosition!.value.copy(sunDir);
  scene.add(sky);
  const pmrem = new THREE.PMREMGenerator(renderer);
  const envScene = new THREE.Scene();
  envScene.add(sky.clone());
  scene.environment = pmrem.fromScene(envScene, 0.02).texture;
  scene.environmentIntensity = 0.7;
  scene.fog = new THREE.FogExp2(0xb9c4cc, 0.0065);

  const sun = new THREE.DirectionalLight(0xfff0dc, 3.4);
  sun.castShadow = true;
  sun.shadow.mapSize.set(4096, 4096);
  sun.shadow.bias = -0.0003;
  sun.shadow.normalBias = 0.03;
  Object.assign(sun.shadow.camera, { left: -45, right: 45, top: 45, bottom: -45, near: 1, far: 220 });
  sun.shadow.camera.updateProjectionMatrix();
  scene.add(sun, sun.target);

  const maxAniso = renderer.capabilities.getMaxAnisotropy();
  const mats = mapMaterials();
  for (const m of mats) for (const t of [m.material.map, m.material.normalMap]) if (t) t.anisotropy = maxAniso;

  // one merged mesh per material
  const byMat = new Map<number, THREE.BufferGeometry[]>();
  for (const b of boxes) {
    const spec = mats[b.mat] ?? mats[0]!;
    if (!byMat.has(b.mat)) byMat.set(b.mat, []);
    byMat.get(b.mat)!.push(boxGeometry(b, spec.tile));
  }
  for (const [mat, geoms] of byMat) {
    const mesh = new THREE.Mesh(mergeGeometries(geoms), (mats[mat] ?? mats[0]!).material);
    mesh.castShadow = mesh.receiveShadow = true;
    scene.add(mesh);
  }

  // asphalt yard, gravel beyond the walls
  const g = groundMaterial();
  for (const t of [g.material.map, g.material.normalMap]) if (t) { t.anisotropy = maxAniso; t.repeat.set((half * 2) / g.tile, (half * 2) / g.tile); }
  const ground = new THREE.Mesh(new THREE.PlaneGeometry(half * 2, half * 2), g.material);
  ground.rotation.x = -Math.PI / 2;
  ground.receiveShadow = true;
  scene.add(ground);
  const outside = new THREE.Mesh(new THREE.PlaneGeometry(1200, 1200), new THREE.MeshStandardMaterial({ color: 0x6e6658, roughness: 1 }));
  outside.rotation.x = -Math.PI / 2;
  outside.position.y = -0.02;
  scene.add(outside);

  // painted parking lines for scale
  const lineMat = new THREE.MeshStandardMaterial({ color: 0xd8d2b8, roughness: 0.8 });
  for (let i = -3; i <= 3; i++) {
    const l = new THREE.Mesh(new THREE.PlaneGeometry(0.12, 5), lineMat);
    l.rotation.x = -Math.PI / 2;
    l.position.set(i * 2.6, 0.005, 26);
    l.receiveShadow = true;
    scene.add(l);
  }

  return {
    sun,
    /** Keep the shadow frustum centred on the player. */
    follow(x: number, z: number) {
      sun.target.position.set(x, 0, z);
      sun.position.copy(sunDir).multiplyScalar(100).add(sun.target.position);
    },
  };
}

/** Distance along a ray to the first box hit (slab test), or `max`. */
export function raycastBoxes(boxes: Box[], o: THREE.Vector3, d: THREE.Vector3, max: number, pad = 0.15) {
  let best = max;
  for (const b of boxes) {
    let t0 = 0, t1 = best;
    let hit = true;
    for (const a of ['x', 'y', 'z'] as const) {
      const lo = b.min[a] - pad, hi = b.max[a] + pad;
      if (Math.abs(d[a]) < 1e-8) { if (o[a] < lo || o[a] > hi) { hit = false; break; } continue; }
      let ta = (lo - o[a]) / d[a], tb = (hi - o[a]) / d[a];
      if (ta > tb) [ta, tb] = [tb, ta];
      t0 = Math.max(t0, ta); t1 = Math.min(t1, tb);
      if (t0 > t1) { hit = false; break; }
    }
    if (hit && t0 < best) best = t0;
  }
  // ground
  if (d.y < 0) best = Math.min(best, (o.y - pad) / -d.y);
  return best;
}
