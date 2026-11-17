import * as THREE from 'three';
import { GLTFLoader, type GLTF } from 'three/examples/jsm/loaders/GLTFLoader.js';
import * as SkeletonUtils from 'three/examples/jsm/utils/SkeletonUtils.js';

/**
 * Realistic soldier: skinned mesh with motion-captured Idle/Walk/Run clips, blended by
 * speed, plus procedural layers on top of the clips (crouch, strafe hip twist,
 * aim pitch, lean, slap swing, stagger, knockout).
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
}

const BONES = ['Hips', 'Spine', 'Spine1', 'Spine2', 'Neck', 'Head', 'RightArm', 'RightForeArm', 'RightHand', 'LeftArm',
  'LeftUpLeg', 'LeftLeg', 'LeftFoot', 'RightUpLeg', 'RightLeg', 'RightFoot'] as const;
type BoneName = (typeof BONES)[number];

const UP = new THREE.Vector3(0, 1, 0);
const tq = new THREE.Quaternion();
const tq2 = new THREE.Quaternion();
const tv = new THREE.Vector3();
const ease = (t: number) => (t <= 0 ? 0 : t >= 1 ? 1 : t * t * (3 - 2 * t));
const damp = (cur: number, target: number, rate: number, dt: number) => cur + (target - cur) * (1 - Math.exp(-rate * dt));

/** Rotate a bone about a world-space axis, on top of whatever the clips set. */
function rotateWorld(bone: THREE.Object3D, axis: THREE.Vector3, angle: number) {
  if (Math.abs(angle) < 1e-5) return;
  bone.parent!.getWorldQuaternion(tq);
  tq2.setFromAxisAngle(axis, angle);
  // local delta = parent^-1 * worldRot * parent
  const d = tq.clone().invert().multiply(tq2).multiply(tq);
  bone.quaternion.premultiply(d);
  bone.updateMatrixWorld(true);
}

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
  private visY = 0;
  private time = Math.random() * 10;

  constructor(scene: THREE.Scene, tint?: THREE.Color) {
    if (!gltf) throw new Error('loadCharacter() first');
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
        if (!/visor/i.test(m.name)) mat.color.copy(new THREE.Color(1, 1, 1).lerp(tint, 0.35));
        m.material = mat;
      }
    });
    for (const n of BONES) {
      const b = this.model.getObjectByName('mixamorig' + n) as THREE.Bone;
      this.bones[n] = b;
      this.rest.set(b, b.quaternion.clone());
    }
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
  }

  slap() { this.slapT = 0; }

  set visible(v: boolean) { this.model.visible = v; }

  dispose(scene: THREE.Scene) {
    scene.remove(this.root);
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
    this.koW = damp(this.koW, p.ko > 0 ? 1 : 0, p.ko > 0 ? 7 : 3, dt);
    this.stunW = damp(this.stunW, p.stun > 0 || p.ko > 0 ? 1 : 0, 12, dt);

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
      }
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
  }
}
