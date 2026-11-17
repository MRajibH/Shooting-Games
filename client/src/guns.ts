import * as THREE from 'three';
import { weaponsJson } from './wasm/riftline_sim.js';
import type { WeaponSpec } from './protocol';

let table: WeaponSpec[] | null = null;
/** Weapon table, straight from the Rust sim so both sides agree (after WASM init). */
export function weapons(): WeaponSpec[] {
  return (table ??= JSON.parse(weaponsJson()) as WeaponSpec[]);
}
export const spec = (w: number) => weapons()[Math.max(0, Math.min(3, w))]!;

/**
 * Procedural weapon models built from simple solids at real dimensions.
 * Local frame: -Z is the barrel direction, +Y up, origin at the pistol grip
 * (where the right hand holds). `fore` is where the left hand goes, `muzzle` the
 * barrel end, `magwell` where magazines or shells go in.
 */
export interface GunModel {
  group: THREE.Group;
  muzzle: THREE.Vector3;
  fore: THREE.Vector3;
  magwell: THREE.Vector3;
  /** Detachable magazine (moves with the hand during a reload), if any. */
  mag: THREE.Object3D | null;
  /** Sliding part (pump forend or bolt) animated after each shot. */
  action: THREE.Object3D | null;
  actionTravel: THREE.Vector3;
}

const mats = (() => {
  const m = (color: number, roughness: number, metalness: number) => new THREE.MeshStandardMaterial({ color, roughness, metalness });
  return {
    polymer: m(0x26282b, 0.75, 0.05),
    tan: m(0x8a7a5c, 0.8, 0.05),
    metal: m(0x34373b, 0.38, 0.85),
    darkMetal: m(0x1c1d1f, 0.45, 0.8),
    wood: m(0x6a4527, 0.62, 0.0),
    glass: new THREE.MeshStandardMaterial({ color: 0x0d1820, roughness: 0.05, metalness: 0.2, emissive: 0x05101a }),
  };
})();

function box(g: THREE.Object3D, mat: THREE.Material, sx: number, sy: number, sz: number, x: number, y: number, z: number, rx = 0) {
  const m = new THREE.Mesh(new THREE.BoxGeometry(sx, sy, sz), mat);
  m.position.set(x, y, z);
  m.rotation.x = rx;
  m.castShadow = true;
  g.add(m);
  return m;
}

/** Cylinder along Z from z0 to z1. */
function tube(g: THREE.Object3D, mat: THREE.Material, r: number, z0: number, z1: number, y: number, x = 0, seg = 14) {
  const m = new THREE.Mesh(new THREE.CylinderGeometry(r, r, Math.abs(z1 - z0), seg), mat);
  m.rotation.x = Math.PI / 2;
  m.position.set(x, y, (z0 + z1) / 2);
  m.castShadow = true;
  g.add(m);
  return m;
}

function carbine(): GunModel {
  const g = new THREE.Group();
  const { polymer, metal, darkMetal, tan, glass } = mats;
  box(g, metal, 0.05, 0.065, 0.3, 0, 0.055, -0.06);              // receiver
  box(g, darkMetal, 0.022, 0.012, 0.3, 0, 0.094, -0.08);           // top rail
  box(g, tan, 0.058, 0.06, 0.24, 0, 0.055, -0.33);                 // handguard
  tube(g, darkMetal, 0.009, -0.45, -0.6, 0.055);                   // barrel
  tube(g, darkMetal, 0.013, -0.6, -0.66, 0.055);                   // flash hider
  box(g, darkMetal, 0.012, 0.05, 0.012, 0, 0.09, -0.52);           // front sight post
  box(g, polymer, 0.03, 0.1, 0.045, 0, -0.03, 0.02, -0.3);         // pistol grip
  box(g, polymer, 0.02, 0.04, 0.24, 0, 0.05, 0.2);                 // buffer tube
  box(g, tan, 0.045, 0.085, 0.13, 0, 0.035, 0.27);                 // stock
  // red-dot sight
  tube(g, darkMetal, 0.02, -0.02, -0.1, 0.13);
  const lens = tube(g, glass, 0.016, -0.1, -0.101, 0.13);
  lens.scale.set(1, 1, 1);
  box(g, darkMetal, 0.02, 0.025, 0.05, 0, 0.105, -0.06);
  const mag = new THREE.Group();
  box(mag, polymer, 0.026, 0.16, 0.07, 0, -0.07, 0, 0.18);
  mag.position.set(0, 0.0, -0.13);
  g.add(mag);
  return { group: g, muzzle: new THREE.Vector3(0, 0.055, -0.67), fore: new THREE.Vector3(0, 0.02, -0.33),
    magwell: new THREE.Vector3(0, -0.02, -0.13), mag, action: null, actionTravel: new THREE.Vector3() };
}

function smg(): GunModel {
  const g = new THREE.Group();
  const { polymer, metal, darkMetal } = mats;
  box(g, metal, 0.05, 0.07, 0.3, 0, 0.05, -0.1);                   // receiver
  box(g, polymer, 0.055, 0.055, 0.14, 0, 0.045, -0.26);            // handguard
  tube(g, darkMetal, 0.009, -0.33, -0.4, 0.05);                    // barrel
  tube(g, darkMetal, 0.016, -0.36, -0.42, 0.05);                   // muzzle device
  box(g, polymer, 0.03, 0.1, 0.045, 0, -0.03, 0.02, -0.3);         // grip
  box(g, darkMetal, 0.012, 0.07, 0.16, 0, 0.04, 0.14);             // folding stock frame
  box(g, polymer, 0.04, 0.08, 0.03, 0, 0.035, 0.22);               // butt pad
  box(g, darkMetal, 0.018, 0.02, 0.2, 0, 0.095, -0.12);            // rail
  tube(g, darkMetal, 0.015, -0.06, -0.12, 0.12);                   // sight
  const mag = new THREE.Group();
  box(mag, polymer, 0.024, 0.18, 0.035, 0, -0.08, 0, 0.05);
  mag.position.set(0, 0.0, -0.17);
  g.add(mag);
  return { group: g, muzzle: new THREE.Vector3(0, 0.05, -0.43), fore: new THREE.Vector3(0, 0.015, -0.25),
    magwell: new THREE.Vector3(0, -0.02, -0.17), mag, action: null, actionTravel: new THREE.Vector3() };
}

function shotgun(): GunModel {
  const g = new THREE.Group();
  const { metal, darkMetal, wood } = mats;
  box(g, metal, 0.048, 0.07, 0.2, 0, 0.05, -0.05);                 // receiver
  tube(g, darkMetal, 0.012, -0.15, -0.72, 0.07);                   // barrel
  tube(g, darkMetal, 0.011, -0.15, -0.62, 0.042);                  // magazine tube
  box(g, darkMetal, 0.01, 0.012, 0.01, 0, 0.088, -0.7);            // bead sight
  box(g, wood, 0.035, 0.09, 0.05, 0, -0.02, 0.07, -0.45);          // grip/wrist
  const stock = box(g, wood, 0.042, 0.1, 0.3, 0, 0.0, 0.25);       // stock
  stock.rotation.x = 0.12;
  const pump = new THREE.Group();
  box(pump, wood, 0.05, 0.045, 0.18, 0, 0.045, 0);
  pump.position.set(0, 0, -0.33);
  g.add(pump);
  return { group: g, muzzle: new THREE.Vector3(0, 0.07, -0.73), fore: new THREE.Vector3(0, 0.02, -0.33),
    magwell: new THREE.Vector3(0, 0.0, -0.08), mag: null, action: pump, actionTravel: new THREE.Vector3(0, 0, 0.11) };
}

function boltRifle(): GunModel {
  const g = new THREE.Group();
  const { polymer, darkMetal, metal, glass } = mats;
  const green = new THREE.MeshStandardMaterial({ color: 0x3d4535, roughness: 0.8 });
  box(g, metal, 0.048, 0.06, 0.26, 0, 0.055, -0.08);               // action
  box(g, green, 0.06, 0.07, 0.42, 0, 0.03, -0.3);                  // fore-end
  tube(g, darkMetal, 0.012, -0.5, -0.95, 0.06);                    // heavy barrel
  tube(g, darkMetal, 0.018, -0.95, -1.03, 0.06);                   // muzzle brake
  box(g, green, 0.035, 0.1, 0.05, 0, -0.03, 0.03, -0.25);          // grip
  box(g, green, 0.045, 0.12, 0.3, 0, 0.03, 0.24);                  // stock
  box(g, green, 0.03, 0.035, 0.12, 0, 0.11, 0.2);                  // cheek rest
  // scope: tube, bells, turrets, lenses
  tube(g, darkMetal, 0.016, 0.06, -0.24, 0.135);
  tube(g, darkMetal, 0.026, -0.24, -0.34, 0.135, 0, 18);
  tube(g, darkMetal, 0.021, 0.06, 0.12, 0.135, 0, 18);
  tube(g, glass, 0.023, -0.34, -0.341, 0.135, 0, 18);
  box(g, darkMetal, 0.02, 0.03, 0.02, 0, 0.16, -0.09);
  box(g, darkMetal, 0.03, 0.02, 0.02, 0.025, 0.135, -0.09);
  box(g, darkMetal, 0.02, 0.03, 0.02, 0, 0.09, -0.04);             // scope mounts
  box(g, darkMetal, 0.02, 0.03, 0.02, 0, 0.09, -0.16);
  const bolt = new THREE.Group();
  const handle = box(bolt, metal, 0.06, 0.012, 0.012, 0.045, 0, 0);
  handle.rotation.z = -0.4;
  const knob = new THREE.Mesh(new THREE.SphereGeometry(0.012, 10, 8), metal);
  knob.position.set(0.075, -0.012, 0);
  bolt.add(knob);
  bolt.position.set(0, 0.07, 0.02);
  g.add(bolt);
  const mag = new THREE.Group();
  box(mag, polymer, 0.03, 0.06, 0.08, 0, -0.025, 0);
  mag.position.set(0, 0.0, -0.1);
  g.add(mag);
  return { group: g, muzzle: new THREE.Vector3(0, 0.06, -1.04), fore: new THREE.Vector3(0, 0.0, -0.36),
    magwell: new THREE.Vector3(0, -0.01, -0.1), mag, action: bolt, actionTravel: new THREE.Vector3(0, 0, 0.09) };
}

const builders = [carbine, smg, shotgun, boltRifle];

export function buildGun(weapon: number): GunModel {
  const m = builders[Math.max(0, Math.min(3, weapon))]!();
  m.group.traverse((o) => { if ((o as THREE.Mesh).isMesh) (o as THREE.Mesh).receiveShadow = true; });
  return m;
}
