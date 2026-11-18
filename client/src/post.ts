import * as THREE from 'three';
import { EffectComposer } from 'three/examples/jsm/postprocessing/EffectComposer.js';
import { GTAOPass } from 'three/examples/jsm/postprocessing/GTAOPass.js';
import { OutputPass } from 'three/examples/jsm/postprocessing/OutputPass.js';
import { RenderPass } from 'three/examples/jsm/postprocessing/RenderPass.js';
import { UnrealBloomPass } from 'three/examples/jsm/postprocessing/UnrealBloomPass.js';

export type Tier = 'high' | 'medium' | 'low';

/**
 * Post-processing by quality tier. High: ambient occlusion (GTAO, contact shadows in
 * corners and under cover) plus a restrained bloom that only catches very bright
 * things (muzzle flashes, sun glints). Medium: bloom only. Low: straight render.
 * Rendering goes through a multisampled HDR target so edges stay anti-aliased.
 */
export class Post {
  private composer: EffectComposer;
  private gtao: GTAOPass;
  private bloom: UnrealBloomPass;
  private tier: Tier = 'high';

  constructor(private renderer: THREE.WebGLRenderer, private scene: THREE.Scene, private camera: THREE.PerspectiveCamera) {
    const size = renderer.getSize(new THREE.Vector2());
    const target = new THREE.WebGLRenderTarget(size.x, size.y, { type: THREE.HalfFloatType, samples: 4 });
    this.composer = new EffectComposer(renderer, target);
    this.composer.addPass(new RenderPass(scene, camera));
    this.gtao = new GTAOPass(scene, camera, size.x, size.y);
    this.gtao.updateGtaoMaterial({ radius: 0.6, distanceExponent: 1.5, thickness: 1, scale: 1, samples: 12 });
    this.gtao.blendIntensity = 0.85;
    this.composer.addPass(this.gtao);
    // threshold is in linear HDR before tone mapping: the sky and lit walls stay below it
    this.bloom = new UnrealBloomPass(size, 0.35, 0.45, 2.2);
    this.composer.addPass(this.bloom);
    this.composer.addPass(new OutputPass());
  }

  setTier(t: Tier) {
    this.tier = t;
    this.gtao.enabled = t === 'high';
    this.bloom.enabled = t !== 'low';
  }

  /** Match the renderer after a resize or a pixel-ratio change. */
  sync() {
    const size = this.renderer.getSize(new THREE.Vector2());
    this.composer.setPixelRatio(this.renderer.getPixelRatio());
    this.composer.setSize(size.x, size.y);
  }

  render() {
    if (this.tier === 'low') this.renderer.render(this.scene, this.camera);
    else this.composer.render();
  }
}
