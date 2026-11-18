import * as THREE from 'three';
import { GLTFLoader, type GLTF } from 'three/examples/jsm/loaders/GLTFLoader.js';
import * as SkeletonUtils from 'three/examples/jsm/utils/SkeletonUtils.js';
import { buildGun, type GunModel } from './guns';

/**
 * Realistic soldier: skinned mesh with motion-captured Idle/Walk/Run clips, blended by
 * speed, plus procedural layers on top of the clips (crouch, strafe hip twist,
 * aim pitch, lean, slap swing, stagger, knockout). Armed, both arms are solved with
 * two-bone IK onto the weapon (grip and handguard), with hands turned and fingers
 * curled around it, and the weapon follows aim, sprint, reload and swap states.
 */

let gltf: GLTF | null = null;
let yawOffset = 0;
let scale = 1;

export async function loadCharacter(url: string) {
  gltf = await new GLTFLoader().loadAsync(url);
  const s = gltf.scene;
  s.updateMatrixWorld(true);
  // Find which way the model faces from its foot -> toe vector, so it can be turned to -Z.
  const foot = s.getObjectByName('mixamorigLeftFoot')!.getWorldPosition(new THREE.Vector3());
  const toe = s.getObjectByName('mixamorigLeftToeBase')!.getWorldPosition(new THREE.Vector3());
  yawOffset = Math.PI - Math.atan2(toe.x - foot.x, toe.z - foot.z);
  const head = s.getObjectByName('mixamorigHeadTop_End')!.getWorldPosition(new THREE.Vector3());
  const box = new THREE.Box3().setFromObject(s);
  scale = 1.8 / Math.max(0.5, head.y - box.min.y);
}

export interface Pose {
  x: number; y: number; z: number;
  vx: number; vz: number;
  yaw: number; pitch: number;
  crouched: boolean; grounded: boolean;
  lean: number; stun: number; ko: number;
  armed?: boolean; weapon?: number; ads?: number; reload?: number; reloadLen?: number;
  swap?: number; lowered?: number; dead?: number;
}

const BONES = ['Hips', 'Spine', 'Spine1', 'Spine2', 'Neck', 'Head', 'RightShoulder', 'RightArm', 'RightForeArm', 'RightHand',
  'LeftShoulder', 'LeftArm', 'LeftForeArm', 'LeftHand',
  'LeftUpLeg', 'LeftLeg', 'LeftFoot', 'RightUpLeg', 'RightLeg', 'RightFoot'] as const;
const FINGERS = ['Index', 'Middle', 'Ring', 'Pinky'] as const;
const SWAP_TIME = 0.6;
type BoneName = (typeof BONES)[number];

const UP = new THREE.Vector3(0, 1, 0);
const tq = new THREE.Quaternion();
const tq2 = new THREE.Quaternion();
const tv = new THREE.Vector3();
const ease = (t: number) => (t <= 0 ? 0 : t >= 1 ? 1 : t * t * (3 - 2 * t));
const damp = (cur: number, target: number, rate: number, dt: number) => cur + (target - cur) * (1 - Math.exp(-rate * dt));

/** Apply a world-space rotation to a bone (about its own pivot). */
function applyWorld(bone: THREE.Object3D, q: THREE.Quaternion) {
  bone.parent!.getWorldQuaternion(tq);
  // local delta = parent^-1 * worldRot * parent
  const d = tq.clone().invert().multiply(q).multiply(tq);
  bone.quaternion.premultiply(d);
  bone.updateMatrixWorld(true);
}

/** Rotate a bone about a world-space axis, on top of whatever the clips set. */
function rotateWorld(bone: THREE.Object3D, axis: THREE.Vector3, angle: number) {
  if (Math.abs(angle) < 1e-5) return;
  applyWorld(bone, tq2.setFromAxisAngle(axis, angle));
}

const wp = (o: THREE.Object3D) => o.getWorldPosition(new THREE.Vector3());

/** Turn a bone so the world direction `from` (measured from it) points along `to`. */
function aimBone(bone: THREE.Object3D, from: THREE.Vector3, to: THREE.Vector3) {
  if (from.lengthSq() < 1e-10 || to.lengthSq() < 1e-10) return;
  applyWorld(bone, new THREE.Quaternion().setFromUnitVectors(from.clone().normalize(), to.clone().normalize()));
}

/** Analytic two-bone IK: put `hand` (the wrist) on `target`, elbow bending toward `pole`. */
function twoBoneIK(upper: THREE.Bone, lower: THREE.Bone, hand: THREE.Bone, target: THREE.Vector3, pole: THREE.Vector3) {
  const a = wp(upper), b = wp(lower), c = wp(hand);
  const l1 = a.distanceTo(b), l2 = b.distanceTo(c);
  const toT = target.clone().sub(a);
  const d = THREE.MathUtils.clamp(toT.length(), Math.abs(l1 - l2) + 1e-3, (l1 + l2) * 0.999);
  const dir = toT.normalize();
  const side = pole.clone().sub(a);
  side.addScaledVector(dir, -side.dot(dir));
  if (side.lengthSq() < 1e-8) side.set(0, -1, 0);
  side.normalize();
  const cosA = (l1 * l1 + d * d - l2 * l2) / (2 * l1 * d);
  const sinA = Math.sqrt(Math.max(0, 1 - cosA * cosA));
  const elbow = a.clone().addScaledVector(dir, cosA * l1).addScaledVector(side, sinA * l1);
  aimBone(upper, b.clone().sub(a), elbow.sub(a));
  const b2 = wp(lower), c2 = wp(hand);
  aimBone(lower, c2.sub(b2), a.addScaledVector(dir, d).sub(b2));
}

interface HandRig { hand: THREE.Bone; index1: THREE.Bone; middle1: THREE.Bone; pinky1: THREE.Bone; thumb1: THREE.Bone;
  fingers: THREE.Bone[][]; palmSign: number }

export class Character {
  readonly root = new THREE.Group();
  private tilt = new THREE.Group();
  private model: THREE.Object3D;
  private mixer: THREE.AnimationMixer;
  private idle: THREE.AnimationAction;
  private walk: THREE.AnimationAction;
  private run: THREE.AnimationAction;
  private bones = {} as Record<BoneName, THREE.Bone>;
  private rest = new Map<THREE.Bone, THREE.Quaternion>();
  private footBase = 0;
  private crouchW = 0;
  private koW = 0;
  private stunW = 0;
  private hipYaw = 0;
  private slapT = -1;
  /** Face being slapped, if any: the hand is steered onto it. */
  private slapTarget: (() => THREE.Vector3 | null) | null = null;
  /** Head snap from taking a slap: 1 at impact, decaying to 0. */
  private jerk = 0;
  private jerkSign = 1;
  private visY = 0;
  private time = Math.random() * 10;
  private hands!: { left: HandRig; right: HandRig };
  private scene: THREE.Scene;
  private gun: GunModel | null = null;
  private gunWeapon = -1;
  private kickT = 1;
  private aimW = 0;
  private sprintW = 0;
  private gunVisible = true;
  /** Muzzle position in the world (valid after update). */
  readonly muzzle = new THREE.Vector3();
  readonly aimDir = new THREE.Vector3(0, 0, -1);

  constructor(scene: THREE.Scene, tint?: THREE.Color, strength = 0.35) {
    if (!gltf) throw new Error('loadCharacter() first');
    this.scene = scene;
    this.model = SkeletonUtils.clone(gltf.scene);
    this.model.scale.setScalar(scale);
    this.model.rotation.y = yawOffset;
    this.model.traverse((o) => {
      const m = o as THREE.SkinnedMesh;
      if (!m.isMesh) return;
      m.castShadow = true;
      m.receiveShadow = true;
      m.frustumCulled = false;
      if (tint) {
        const mat = (m.material as THREE.MeshStandardMaterial).clone();
        if (!/visor/i.test(m.name)) mat.color.copy(new THREE.Color(1, 1, 1).lerp(tint, strength));
        m.material = mat;
      }
    });
    for (const n of BONES) {
      const b = this.model.getObjectByName('mixamorig' + n) as THREE.Bone;
      this.bones[n] = b;
      this.rest.set(b, b.quaternion.clone());
    }
    const rig = (side: 'Left' | 'Right'): HandRig => {
      const g = (n: string) => this.model.getObjectByName(`mixamorig${side}Hand${n}`) as THREE.Bone;
      const fingers = FINGERS.map((f) => [1, 2, 3].map((k) => g(`${f}${k}`)));
      for (const b of [...fingers.flat(), g('Thumb1'), g('Thumb2')]) this.rest.set(b, b.quaternion.clone());
      return { hand: this.bones[`${side}Hand`], index1: g('Index1'), middle1: g('Middle1'), pinky1: g('Pinky1'),
        thumb1: g('Thumb1'), fingers, palmSign: 1 };
    };
    this.hands = { left: rig('Left'), right: rig('Right') };
    this.tilt.add(this.model);
    this.root.add(this.tilt);
    scene.add(this.root);

    this.mixer = new THREE.AnimationMixer(this.model);
    const clip = (name: string) => this.mixer.clipAction(gltf!.animations.find((a) => a.name === name)!);
    this.idle = clip('Idle');
    this.walk = clip('Walk');
    this.run = clip('Run');
    for (const a of [this.idle, this.walk, this.run]) { a.play(); a.setEffectiveWeight(0); }
    this.idle.setEffectiveWeight(1);

    this.mixer.update(0);
    this.root.updateMatrixWorld(true);
    this.footBase = this.footHeight();
    // which side of the hand is the palm: the side fingers curl toward
    for (const h of [this.hands.left, this.hands.right]) {
      const across = wp(h.index1).sub(wp(h.pinky1)).normalize();
      const dirF = wp(h.middle1).sub(wp(h.hand)).normalize();
      const tip = h.fingers[1]![2]!;
      const before = wp(tip);
      const q = h.middle1.quaternion.clone();
      rotateWorld(h.middle1, across, 0.6);
      const moved = wp(tip).sub(before);
      h.middle1.quaternion.copy(q);
      h.middle1.updateMatrixWorld(true);
      h.palmSign = Math.sign(moved.dot(new THREE.Vector3().crossVectors(dirF, across))) || 1;
    }
  }

  /** Where spent cases come out (right side of the receiver), and the gun's right. */
  ejectPort(): { at: THREE.Vector3; right: THREE.Vector3 } | null {
    if (!this.gun || !this.gun.group.visible) return null;
    const g = this.gun.group;
    return {
      at: new THREE.Vector3(0.035, 0.07, -0.06).applyMatrix4(g.matrixWorld),
      right: new THREE.Vector3(1, 0.3, 0.2).normalize().applyQuaternion(g.quaternion),
    };
  }

  /** Recoil kick on the model (and pump / bolt cycling). */
  fire() { this.kickT = 0; }

  private setGun(weapon: number) {
    if (this.gunWeapon === weapon && this.gun) return;
    if (this.gun) this.scene.remove(this.gun.group);
    this.gun = buildGun(weapon);
    this.gunWeapon = weapon;
    this.gun.group.visible = this.gunVisible;
    this.scene.add(this.gun.group);
  }

  /** Point a hand's fingers along `fingerDir` with the palm facing `palmDir`, then curl them. */
  private grip(h: HandRig, fingerDir: THREE.Vector3, palmDir: THREE.Vector3, curl: number, trigger = curl) {
    aimBone(h.hand, wp(h.middle1).sub(wp(h.hand)), fingerDir);
    const dirF = wp(h.middle1).sub(wp(h.hand)).normalize();
    const across = wp(h.index1).sub(wp(h.pinky1)).normalize();
    const palm = new THREE.Vector3().crossVectors(dirF, across).multiplyScalar(h.palmSign).normalize();
    const want = palmDir.clone().addScaledVector(dirF, -palmDir.dot(dirF)).normalize();
    const angle = Math.atan2(new THREE.Vector3().crossVectors(palm, want).dot(dirF), palm.dot(want));
    rotateWorld(h.hand, dirF, angle);
    const ax = wp(h.index1).sub(wp(h.pinky1)).normalize();
    h.fingers.forEach((segs, i) => {
      const c = i === 0 ? trigger : curl;
      for (const b of segs) rotateWorld(b, ax, c * h.palmSign);
    });
    rotateWorld(h.thumb1, dirF, 0.4 * curl * h.palmSign);
  }

  /** Swing a slap; with a target, the open hand lands across their face. */
  slap(target: (() => THREE.Vector3 | null) | null = null) { this.slapT = 0; this.slapTarget = target; }

  /** Took a slap pushing along world direction `push`: the head snaps that way. */
  slapped(push: THREE.Vector3) {
    const yaw = this.root.rotation.y;
    const fwd = new THREE.Vector3(-Math.sin(yaw), 0, -Math.cos(yaw));
    // which way round the neck the push turns the face
    this.jerkSign = Math.sign(fwd.x * push.z - fwd.z * push.x) || 1;
    this.jerk = 1;
  }

  /** Centre of the face in the world (valid after update). */
  facePos() {
    const head = this.bones.Head.getWorldPosition(new THREE.Vector3());
    const yaw = this.root.rotation.y;
    return head.add(new THREE.Vector3(-Math.sin(yaw) * 0.1, 0.07, -Math.cos(yaw) * 0.1));
  }

  set visible(v: boolean) {
    this.model.visible = v;
    this.gunVisible = v;
    if (this.gun) this.gun.group.visible = v;
  }

  dispose(scene: THREE.Scene) {
    scene.remove(this.root);
    if (this.gun) scene.remove(this.gun.group);
    this.mixer.stopAllAction();
  }

  private footHeight() {
    const l = this.bones.LeftFoot.getWorldPosition(tv).y;
    const r = this.bones.RightFoot.getWorldPosition(tv).y;
    return Math.min(l, r) - this.root.position.y;
  }

  update(p: Pose, dt: number) {
    this.time += dt;
    const speed = Math.hypot(p.vx, p.vz);
    const sin = Math.sin(p.yaw), cos = Math.cos(p.yaw);
    const fwd = p.vx * -sin + p.vz * -cos;
    const side = p.vx * cos + p.vz * -sin;

    // root follows the sim; smooth small vertical steps (stairs) only
    this.visY = Math.abs(p.y - this.visY) > 0.6 ? p.y : damp(this.visY, p.y, 18, dt);
    this.root.position.set(p.x, this.visY, p.z);
    this.root.rotation.y = p.yaw;

    this.crouchW = damp(this.crouchW, p.crouched ? 1 : 0, 10, dt);
    const down = p.ko > 0 || (p.dead ?? 0) > 0;
    this.koW = damp(this.koW, down ? 1 : 0, down ? 7 : 3, dt);
    this.stunW = damp(this.stunW, p.stun > 0 || p.ko > 0 ? 1 : 0, 12, dt);
    const armed = !!p.armed && !down;
    this.aimW = damp(this.aimW, armed ? 1 : 0, 10, dt);
    this.sprintW = damp(this.sprintW, armed && (p.lowered ?? 0) > 0 && speed > 4 ? 1 : 0, 9, dt);

    // reset bones we layer on, so additive rotations never accumulate
    for (const [b, q] of this.rest) b.quaternion.copy(q);

    // locomotion blend: idle -> walk -> run by ground speed
    const moving = p.grounded && this.koW < 0.5 ? speed : 0;
    const wIdle = Math.max(0, 1 - moving / 1.0);
    const wRun = Math.min(1, Math.max(0, (moving - 3.4) / 1.8));
    const wWalk = Math.max(0, 1 - wIdle - wRun);
    this.idle.setEffectiveWeight(wIdle);
    this.walk.setEffectiveWeight(wWalk);
    this.run.setEffectiveWeight(wRun);
    const backwards = fwd < -0.3 && Math.abs(fwd) > Math.abs(side) * 0.5;
    this.walk.timeScale = (backwards ? -1 : 1) * Math.min(2.2, Math.max(0.5, moving / 1.6));
    this.run.timeScale = Math.min(1.4, Math.max(0.7, moving / 5.0));
    this.mixer.update(this.koW > 0.9 ? 0 : dt);
    this.root.updateMatrixWorld(true);

    const B = this.bones;
    const right = tv.set(cos, 0, -sin).clone();
    const forward = new THREE.Vector3(-sin, 0, -cos);

    // strafing: turn the hips toward the movement direction, keep the chest on aim
    let moveAngle = speed > 0.4 ? Math.atan2(side, fwd) : 0;
    if (Math.abs(moveAngle) > Math.PI / 2) moveAngle -= Math.sign(moveAngle) * Math.PI;
    this.hipYaw = damp(this.hipYaw, -THREE.MathUtils.clamp(moveAngle, -1.0, 1.0) * (1 - this.koW), 10, dt);
    rotateWorld(B.Hips, UP, this.hipYaw);
    rotateWorld(B.Spine, UP, -this.hipYaw * 0.5);
    rotateWorld(B.Spine1, UP, -this.hipYaw * 0.5);
    const legRight = right.clone().applyAxisAngle(UP, this.hipYaw);

    // crouch: fold hips, knees and ankles, lean the torso forward
    const c = this.crouchW;
    if (c > 0.01) {
      for (const [up, low, foot] of [[B.LeftUpLeg, B.LeftLeg, B.LeftFoot], [B.RightUpLeg, B.RightLeg, B.RightFoot]] as const) {
        rotateWorld(up, legRight, 1.15 * c);
        rotateWorld(low, legRight, -1.9 * c);
        rotateWorld(foot, legRight, 0.75 * c);
      }
      rotateWorld(B.Spine, right, -0.3 * c);
    }

    // aim pitch through the spine and head, lean through the spine
    const pitch = p.pitch * (1 - this.koW);
    rotateWorld(B.Spine1, right, pitch * 0.25);
    rotateWorld(B.Spine2, right, pitch * 0.25);
    rotateWorld(B.Head, right, pitch * 0.35);
    rotateWorld(B.Spine, forward, p.lean * 0.22);
    rotateWorld(B.Spine1, forward, p.lean * 0.16);

    // slap: wind up out to the side, whip across, recover
    if (this.slapT >= 0) {
      this.slapT += dt;
      const s = this.slapT / 0.45;
      if (s >= 1) this.slapT = -1;
      else {
        const raise = s < 0.3 ? ease(s / 0.3) : s < 0.5 ? 1 : 1 - ease((s - 0.5) / 0.5);
        const swing = s < 0.3 ? -0.35 * ease(s / 0.3) : s < 0.5 ? -0.35 + 1.35 * ease((s - 0.3) / 0.2) : 1 - ease((s - 0.5) / 0.5);
        rotateWorld(B.Spine1, UP, swing * 0.45);
        rotateWorld(B.RightArm, forward, -raise * 1.3);
        rotateWorld(B.RightArm, UP, swing * 1.7);
        rotateWorld(B.RightForeArm, UP, swing * 0.35);
        rotateWorld(B.RightHand, forward, -raise * 0.4);
        const face = this.slapTarget?.() ?? null;
        if (face) this.palmToFace(face, s, raise, forward, right);
      }
    }

    // slapped: the head whips round fast, then comes back slowly
    if (this.jerk > 0) {
      this.jerk = Math.max(0, this.jerk - dt * 2.6);
      const k = this.jerk > 0.85 ? (1 - this.jerk) / 0.15 : this.jerk / 0.85;
      const a = this.jerkSign * ease(k) * 0.95;
      rotateWorld(B.Neck, UP, -a * 0.4);
      rotateWorld(B.Head, UP, -a * 0.6);
      rotateWorld(B.Head, forward, a * 0.3);
      rotateWorld(B.Spine2, UP, -a * 0.15);
    }

    // staggered: wobble; knocked out: fall on the back
    if (this.stunW > 0.01) {
      rotateWorld(B.Spine, forward, Math.sin(this.time * 21) * 0.16 * this.stunW);
      rotateWorld(B.Head, UP, Math.sin(this.time * 15) * 0.3 * this.stunW);
      rotateWorld(B.Head, forward, Math.sin(this.time * 11) * 0.2 * this.stunW);
    }
    this.tilt.rotation.x = this.koW * (Math.PI / 2 - 0.08);
    this.tilt.position.y = this.koW * 0.16;

    // keep feet on the floor when the legs fold
    if (c > 0.01) {
      this.root.updateMatrixWorld(true);
      const drop = (this.footBase - this.footHeight()) * c;
      this.tilt.position.y += drop;
    }

    // weapon last, once the body is where it will be drawn
    if (p.armed) this.setGun(p.weapon ?? 0);
    if (this.gun) {
      this.gun.group.visible = this.gunVisible && this.aimW > 0.3;
      if (this.aimW > 0.3) {
        this.root.updateMatrixWorld(true);
        this.holdWeapon(p, dt, forward, right);
      }
      this.gun.group.updateMatrixWorld(true);
      this.muzzle.copy(this.gun.muzzle).applyMatrix4(this.gun.group.matrixWorld);
    }
  }

  /**
   * Shoulder the weapon along the aim, then solve both arms onto it. Sprinting
   * carries it low across the chest, swapping dips it, reloading tilts it and walks
   * the support hand to the magazine pouch and back.
   */
  private holdWeapon(p: Pose, dt: number, forward: THREE.Vector3, right: THREE.Vector3) {
    const B = this.bones;
    const gun = this.gun!;
    const up = new THREE.Vector3(0, 1, 0);
    const ads = p.ads ?? 0;
    // stand bladed: left shoulder forward, head turned back to the target
    const blade = 0.38 * (1 - this.sprintW);
    rotateWorld(B.Spine1, UP, -blade * 0.5);
    rotateWorld(B.Spine2, UP, -blade * 0.5);
    rotateWorld(B.Neck, UP, blade * 0.6);
    rotateWorld(B.Head, UP, blade * 0.4);

    // aim direction (the model's), with sprint / swap / reload postures
    const pitch = p.pitch;
    const dir = new THREE.Vector3(-Math.sin(p.yaw) * Math.cos(pitch), Math.sin(pitch), -Math.cos(p.yaw) * Math.cos(pitch));
    this.aimDir.copy(dir);
    const swap = Math.min(1, (p.swap ?? 0) / SWAP_TIME);
    const rl = p.reloadLen && p.reload ? 1 - p.reload / p.reloadLen : 0;
    const reloading = (p.reload ?? 0) > 0 ? Math.sin(Math.min(1, rl) * Math.PI) : 0;
    const gunRight = new THREE.Vector3().crossVectors(dir, up).normalize();
    const lowerBy = this.sprintW * 0.85 + swap * 1.1 + reloading * 0.35;
    dir.applyAxisAngle(gunRight, -lowerBy).applyAxisAngle(up, this.sprintW * 0.55);

    // shoulder pocket, a little inside the shoulder joint
    const shoulder = wp(B.RightArm);
    const pocket = shoulder.clone().addScaledVector(right, -0.05 - 0.03 * ads).addScaledVector(up, -0.02 + 0.03 * ads)
      .addScaledVector(forward, 0.04);
    if (this.sprintW > 0.01) pocket.addScaledVector(right, -0.14 * this.sprintW).addScaledVector(up, -0.12 * this.sprintW);

    // recoil: back and up, then settle; pump / bolt cycles a moment later
    this.kickT += dt;
    const kick = Math.exp(-this.kickT * 22) * (this.kickT < 1 ? 1 : 0);
    const kickBack = kick * (gun.action ? 0.06 : 0.035);
    const gq = new THREE.Quaternion().setFromRotationMatrix(new THREE.Matrix4().lookAt(new THREE.Vector3(), dir, up));
    const roll = reloading * (gun.mag ? 0.45 : 0.8);
    gq.multiply(new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(1, 0, 0), kick * 0.12))
      .multiply(new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 0, 1), roll));
    gun.group.quaternion.copy(gq);
    gun.group.position.copy(pocket).addScaledVector(dir, 0.27 - kickBack).addScaledVector(up, -0.05);
    gun.group.updateMatrixWorld(true);
    if (gun.action) {
      const c = this.kickT - 0.15;
      const k = c < 0 ? 0 : c < 0.15 ? c / 0.15 : c < 0.3 ? 1 - (c - 0.15) / 0.15 : 0;
      gun.action.position.copy(gun.action.userData.home ??= gun.action.position.clone()).addScaledVector(gun.actionTravel, k);
    }

    const L = (v: THREE.Vector3) => v.clone().applyMatrix4(gun.group.matrixWorld);
    const gUp = new THREE.Vector3(0, 1, 0).applyQuaternion(gq);
    const gRight = new THREE.Vector3(1, 0, 0).applyQuaternion(gq);
    const gFwd = new THREE.Vector3(0, 0, -1).applyQuaternion(gq);

    // right hand on the pistol grip
    const rFingers = gFwd.clone().multiplyScalar(0.55).addScaledVector(gUp, -0.8).normalize();
    const rWrist = L(new THREE.Vector3(0.035, 0.02, 0.06));
    twoBoneIK(B.RightArm, B.RightForeArm, B.RightHand, rWrist,
      wp(B.RightArm).addScaledVector(up, -1).addScaledVector(right, 0.7).addScaledVector(forward, -0.3));
    this.grip(this.hands.right, rFingers, gRight.clone().negate(), 1.15, 0.55);

    // left hand under the handguard, or out to the pouch and back while reloading
    let lTarget = L(gun.fore.clone().add(new THREE.Vector3(-0.02, -0.04, 0.03)));
    if ((p.reload ?? 0) > 0) {
      const well = L(gun.magwell.clone().add(new THREE.Vector3(-0.03, -0.08, 0.02)));
      const pouch = wp(B.Hips).addScaledVector(right, -0.18).addScaledVector(forward, 0.12).addScaledVector(up, 0.05);
      const r = Math.min(1, rl);
      const lerp = (a: THREE.Vector3, b: THREE.Vector3, t: number) => a.clone().lerp(b, ease(t));
      lTarget = gun.mag
        ? r < 0.2 ? lerp(lTarget, well, r / 0.2)
          : r < 0.45 ? lerp(well, pouch, (r - 0.2) / 0.25)
          : r < 0.75 ? lerp(pouch, well, (r - 0.45) / 0.3)
          : r < 0.85 ? well : lerp(well, lTarget, (r - 0.85) / 0.15)
        : r < 0.5 ? lerp(well, pouch, r / 0.5) : lerp(pouch, well, (r - 0.5) / 0.5);
      if (gun.mag) {
        // the magazine travels with the hand while it is out of the gun
        const out = r > 0.2 && r < 0.75;
        const home = (gun.mag.userData.home ??= gun.mag.position.clone()) as THREE.Vector3;
        if (out) gun.mag.position.copy(gun.group.worldToLocal(lTarget.clone().addScaledVector(up, -0.05)));
        else gun.mag.position.copy(home);
      }
    } else if (gun.mag?.userData.home) gun.mag.position.copy(gun.mag.userData.home as THREE.Vector3);
    const lFingers = gFwd.clone().multiplyScalar(0.55).addScaledVector(gRight, 0.85).normalize();
    twoBoneIK(B.LeftArm, B.LeftForeArm, B.LeftHand, lTarget,
      wp(B.LeftArm).addScaledVector(up, -1).addScaledVector(right, -0.6));
    this.grip(this.hands.left, (p.reload ?? 0) > 0 ? gUp.clone().negate().add(gFwd).normalize() : lFingers, gUp, 0.75);
  }

  /**
   * Steer the slapping arm so the open palm sweeps across the target's face:
   * lunge in, hand from beside their cheek straight through the face, and out.
   */
  private palmToFace(face: THREE.Vector3, s: number, raise: number, forward: THREE.Vector3, right: THREE.Vector3) {
    const B = this.bones;
    const up = new THREE.Vector3(0, 1, 0);
    const shoulder = wp(B.RightArm);
    const flat = face.clone().sub(this.root.position).setY(0).length();
    if (flat > 1.35) return; // out of reach: just the swing
    // step into the swing and lean the chest in, so the hand reaches the face
    this.root.position.addScaledVector(forward, Math.min(0.75, Math.max(0, flat - 0.48)) * raise);
    this.root.updateMatrixWorld(true);
    rotateWorld(B.Spine1, right, -0.35 * raise);
    const toFace = face.clone().sub(wp(B.RightArm)).normalize();
    const strike = s < 0.3 ? 0 : s < 0.5 ? ease((s - 0.3) / 0.2) : 1;
    const w = s < 0.18 ? 0 : s < 0.3 ? ease((s - 0.18) / 0.12) : s < 0.5 ? 1 : 1 - ease((s - 0.5) / 0.14);
    if (w <= 0) return;
    // palm centre: from 22 cm out on my right of their face, across to the other side
    // aim a little into the face: the palm's skin sits a few cm in front of the bones
    const contact = face.clone().addScaledVector(right, 0.2 * (1 - 2 * strike)).addScaledVector(toFace, 0.03);
    const fingers = up.clone().multiplyScalar(0.75).addScaledVector(forward, 0.35).normalize();
    const wrist = contact.addScaledVector(fingers, -0.06).addScaledVector(toFace, -0.02);
    const hand = wp(B.RightHand);
    twoBoneIK(B.RightArm, B.RightForeArm, B.RightHand, hand.lerp(wrist, w),
      shoulder.addScaledVector(up, -1).addScaledVector(right, 0.9).addScaledVector(forward, -0.2));
    this.grip(this.hands.right, fingers, toFace, 0.08 * w, 0.05 * w);
  }
}
