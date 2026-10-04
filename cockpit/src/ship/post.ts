// The Ship's post-processing (theseus-logs): the hologram's glow, written lean so it holds its frame rate on modest
// GPUs (and on a CPU rasteriser). Two full-resolution passes a frame, the fleet's layer and the composite; the bloom
// works at a quarter and an eighth of the size.
//
// - The sea's still parts (the light table, the grid, the rose) and its stars change only when the camera moves: they
//   are drawn into their own target then.
// - The fleet's layer: the scene drawn over black, with the share of the sea still showing through it in alpha
//   (`layered` in `engine.ts`). The composite puts the sea under it by that share, which is exactly the scene drawn
//   over the sea.
// - Bloom: one bright pass over the layer that also shrinks to a quarter, a separable blur there, a second level at an
//   eighth. The glow is the fleet's alone: the sea's faint light (a star is a pixel or two) gave next to none.
// - The composite: the sea with its swell (the waves and the glints, `SEA_SWELL`, theseus-wp2d) under the layer, the
//   two blooms, a faint chromatic aberration (on the glow, where it shows), scanlines, the vignette, and a static
//   grain, in one pass to the screen, four texture reads a pixel.
// - So a frame of the swell alone, with nothing else moving, is the composite alone: the sea's cache, the layer and
//   its glow are kept. A swell that stands still (Calm, `?swell=0`) is added into the sea's cache once instead, and
//   the composite without it is the one before the swell: only a rolling sea pays for its waves every frame.
// - Every target is 8-bit: a CPU rasteriser pays dearly for half floats (11.7 against 20.4 frames a second on the
//   synthetic fleet), and the composite's grain dithers the glow's gradients.
// - Calm mode skips all of it but the sea's copy, the scene drawn straight over it, as before the swell.
import * as THREE from 'three'
import { SEA_SWELL } from './shaders'

const FS_VERT = /* glsl */ `
varying vec2 vUv;
void main() { vUv = uv; gl_Position = vec4(position.xy, 0.0, 1.0); }
`

const COPY_FRAG = /* glsl */ `
uniform sampler2D tSrc;
varying vec2 vUv;
void main() { gl_FragColor = texture2D(tSrc, vUv); }
`

// A still swell (Calm, `?swell=0`), added into the sea's cache once, so no frame works it out again.
const SWELL_FRAG = /* glsl */ `
varying vec2 vUv;
${SEA_SWELL}
void main() { gl_FragColor = vec4(seaSwell(vUv), 1.0); }
`

// Bright pass and quarter-size in one: four bilinear taps cover the 4x4 source texels under each output texel. Each
// tap is thresholded before they are averaged, so a light a few pixels wide keeps its brightness (averaged first,
// it would be diluted below the threshold and never glow).
const BRIGHT_FRAG = /* glsl */ `
uniform sampler2D tSrc;
uniform vec2 uTexel;
uniform float uThreshold;
varying vec2 vUv;
vec3 bright(vec3 c) { return c * smoothstep(uThreshold, uThreshold + 0.3, max(c.r, max(c.g, c.b))); }
void main() {
  vec3 c = bright(texture2D(tSrc, vUv + uTexel * vec2(-1.0, -1.0)).rgb) + bright(texture2D(tSrc, vUv + uTexel * vec2(1.0, -1.0)).rgb)
         + bright(texture2D(tSrc, vUv + uTexel * vec2(-1.0, 1.0)).rgb) + bright(texture2D(tSrc, vUv + uTexel * vec2(1.0, 1.0)).rgb);
  gl_FragColor = vec4(c * 0.25, 1.0);
}
`

// A nine-tap Gaussian in five fetches (linear sampling between texels).
const BLUR_FRAG = /* glsl */ `
uniform sampler2D tSrc;
uniform vec2 uDir;
varying vec2 vUv;
void main() {
  vec3 c = texture2D(tSrc, vUv).rgb * 0.2270270;
  c += texture2D(tSrc, vUv + uDir * 1.3846154).rgb * 0.3162162;
  c += texture2D(tSrc, vUv - uDir * 1.3846154).rgb * 0.3162162;
  c += texture2D(tSrc, vUv + uDir * 3.2307692).rgb * 0.0702703;
  c += texture2D(tSrc, vUv - uDir * 3.2307692).rgb * 0.0702703;
  gl_FragColor = vec4(c, 1.0);
}
`

const DOWN_FRAG = /* glsl */ `
uniform sampler2D tSrc;
uniform vec2 uTexel;
varying vec2 vUv;
void main() {
  vec3 c = texture2D(tSrc, vUv + uTexel * vec2(-0.5, -0.5)).rgb + texture2D(tSrc, vUv + uTexel * vec2(0.5, -0.5)).rgb
         + texture2D(tSrc, vUv + uTexel * vec2(-0.5, 0.5)).rgb + texture2D(tSrc, vUv + uTexel * vec2(0.5, 0.5)).rgb;
  gl_FragColor = vec4(c * 0.25, 1.0);
}
`

const FINAL_FRAG = /* glsl */ `
uniform sampler2D tSea;
uniform sampler2D tLayer;
uniform sampler2D tBloomA;
uniform sampler2D tBloomB;
uniform float uBloom;
uniform float uCA;
uniform float uScan;
uniform float uVig;
uniform float uPixel;
varying vec2 vUv;
${SEA_SWELL}
float hash12(vec2 p) { vec3 p3 = fract(vec3(p.xyx) * 0.1031); p3 += dot(p3, p3.yzx + 33.33); return fract((p3.x + p3.y) * p3.z); }
void main() {
  vec2 c = vUv - 0.5;
  float r2 = dot(c, c);
  // A faint chromatic aberration, growing toward the edges as through old glass.
  vec2 off = c * uCA * (0.4 + r2 * 3.0) * 6.0;
  // The sea and its swell under the fleet's layer: the share of it that shows through (alpha), then the fleet's own
  // light. A rolling swell is worked out here; a still one is in the sea's cache.
  vec4 layer = texture2D(tLayer, vUv);
  vec3 sea = texture2D(tSea, vUv).rgb;
#ifdef ROLLING
  sea += seaSwell(vUv);
#endif
  vec3 col = sea * layer.a + layer.rgb;
  vec3 ga = texture2D(tBloomA, vUv + off).rgb;
  vec3 gb = texture2D(tBloomB, vUv - off).rgb;
  // The glow's red leans outward and its blue inward: the aberration, where the eye looks for it.
  vec3 glow = vec3(ga.r * 0.6 + gb.r * 0.75, (ga.g + gb.g) * 0.65, ga.b * 0.75 + gb.b * 0.6) * 1.55;
  col += glow * uBloom;
  // Scanlines: every third device row, faint.
  float scan = 0.5 + 0.5 * cos(gl_FragCoord.y / uPixel * 2.0943951);
  col *= 1.0 - uScan * scan;
  // The vignette: the lamp's pool on the chart table.
  col *= mix(1.0, smoothstep(0.95, 0.12, r2 * 1.9), uVig);
  // Static grain, which also dithers the gradients.
  col += (hash12(gl_FragCoord.xy) - 0.5) * 0.014;
  gl_FragColor = vec4(max(col, 0.0), 1.0);
}
`

function target(w: number, h: number, type: THREE.TextureDataType): THREE.WebGLRenderTarget {
  return new THREE.WebGLRenderTarget(Math.max(1, w), Math.max(1, h), {
    type, minFilter: THREE.LinearFilter, magFilter: THREE.LinearFilter, depthBuffer: false, stencilBuffer: false,
  })
}

export class Post {
  private fsCam = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1)
  private fsScene = new THREE.Scene()
  private quad: THREE.Mesh
  private mats: Record<'copy' | 'swell' | 'bright' | 'blur' | 'down' | 'final' | 'finalRolling', THREE.ShaderMaterial>
  private sea = target(1, 1, THREE.UnsignedByteType)
  private layer = target(1, 1, THREE.UnsignedByteType)
  private a1 = target(1, 1, THREE.UnsignedByteType)
  private a2 = target(1, 1, THREE.UnsignedByteType)
  private b1 = target(1, 1, THREE.UnsignedByteType)
  private b2 = target(1, 1, THREE.UnsignedByteType)
  private w = 1
  private h = 1
  /** The sea is drawn again before the next frame (the camera moved, the size or the fleet's rose changed). */
  seaDirty = true
  /** The fleet's layer and its glow must be drawn again before a frame may keep them (resized, or Calm drew none). */
  private layerStale = true
  /** The sea's cache holds the swell, stilled: while it stands still (Calm, or `?swell=0`). A rolling one is drawn in
   *  the composite. */
  private seaHasSwell = false
  private black = new THREE.Color(0, 0, 0)
  private clearWas = new THREE.Color()
  bloom = 2.4
  threshold = 0.45

  /** @param swell `SEA_SWELL`'s uniforms, the engine's: its clock, the camera's rays, and the zoom's scales. */
  constructor(swell: Record<string, THREE.IUniform>) {
    const g = new THREE.BufferGeometry()
    g.setAttribute('position', new THREE.Float32BufferAttribute([-1, -1, 0, 3, -1, 0, -1, 3, 0], 3))
    g.setAttribute('uv', new THREE.Float32BufferAttribute([0, 0, 2, 0, 0, 2], 2))
    const m = (frag: string, uniforms: Record<string, THREE.IUniform>) =>
      new THREE.ShaderMaterial({ vertexShader: FS_VERT, fragmentShader: frag, uniforms, depthTest: false, depthWrite: false })
    const swellAdd = m(SWELL_FRAG, { ...swell })
    swellAdd.blending = THREE.AdditiveBlending
    const finalU = {
      ...swell, tSea: { value: null }, tLayer: { value: null }, tBloomA: { value: null }, tBloomB: { value: null },
      uBloom: { value: this.bloom }, uCA: { value: 0.0018 }, uScan: { value: 0.07 }, uVig: { value: 0.55 }, uPixel: { value: 1 },
    }
    this.mats = {
      copy: m(COPY_FRAG, { tSrc: { value: null } }),
      swell: swellAdd,
      bright: m(BRIGHT_FRAG, { tSrc: { value: null }, uTexel: { value: new THREE.Vector2() }, uThreshold: { value: this.threshold } }),
      blur: m(BLUR_FRAG, { tSrc: { value: null }, uDir: { value: new THREE.Vector2() } }),
      down: m(DOWN_FRAG, { tSrc: { value: null }, uTexel: { value: new THREE.Vector2() } }),
      final: m(FINAL_FRAG, finalU),
      finalRolling: m(FINAL_FRAG, finalU),
    }
    // One composite with the rolling swell worked out in it, one without (its swell is in the sea's cache); they share
    // their uniforms.
    this.mats.finalRolling.defines = { ROLLING: '' }
    this.quad = new THREE.Mesh(g, this.mats.final)
    this.quad.frustumCulled = false
    this.fsScene.add(this.quad)
  }

  /** The internal size in device pixels (the canvas's, times the resolution scale). */
  setSize(w: number, h: number, pixel: number) {
    this.w = Math.max(1, Math.round(w))
    this.h = Math.max(1, Math.round(h))
    this.sea.setSize(this.w, this.h)
    this.layer.setSize(this.w, this.h)
    const qw = Math.max(1, Math.round(this.w / 4))
    const qh = Math.max(1, Math.round(this.h / 4))
    this.a1.setSize(qw, qh)
    this.a2.setSize(qw, qh)
    this.b1.setSize(Math.max(1, Math.round(qw / 2)), Math.max(1, Math.round(qh / 2)))
    this.b2.setSize(Math.max(1, Math.round(qw / 2)), Math.max(1, Math.round(qh / 2)))
    this.mats.final.uniforms.uPixel.value = pixel
    this.seaDirty = true
    this.layerStale = true
  }

  private pass(r: THREE.WebGLRenderer, mat: THREE.ShaderMaterial, to: THREE.WebGLRenderTarget | null, clear = true) {
    this.quad.material = mat
    r.setRenderTarget(to)
    if (clear) r.clear()
    r.render(this.fsScene, this.fsCam)
  }

  /** A frame. `full`: the scene changed or moves, so the fleet's layer and its glow are drawn again; otherwise (the
   *  swell alone) only the composite is. `rolling`: the swell moves (Live mode, not `?swell=0`). */
  render(r: THREE.WebGLRenderer, sea: THREE.Scene, scene: THREE.Scene, camera: THREE.Camera, calm: boolean, full = true, rolling = !calm) {
    const auto = r.autoClear
    r.autoClear = false
    if (rolling && this.seaHasSwell) this.seaDirty = true
    if (this.seaDirty) {
      r.setRenderTarget(this.sea)
      r.clear()
      r.render(sea, camera)
      this.seaDirty = false
      this.seaHasSwell = false
    }
    // A still swell goes into the sea's cache once, so no frame works it out again.
    if (!rolling && !this.seaHasSwell) {
      this.pass(r, this.mats.swell, this.sea, false)
      this.seaHasSwell = true
    }
    if (calm) {
      // Calm: the sea's copy and the scene, straight to the screen.
      const copy = this.mats.copy
      copy.uniforms.tSrc.value = this.sea.texture
      this.pass(r, copy, null)
      r.render(scene, camera)
      this.layerStale = true
      r.autoClear = auto
      return
    }
    if (full || this.layerStale) this.drawLayer(r, scene, camera)
    const fin = rolling ? this.mats.finalRolling : this.mats.final
    fin.uniforms.tSea.value = this.sea.texture
    fin.uniforms.tLayer.value = this.layer.texture
    fin.uniforms.tBloomA.value = this.a1.texture
    fin.uniforms.tBloomB.value = this.b1.texture
    fin.uniforms.uBloom.value = this.bloom
    this.pass(r, fin, null)
    r.autoClear = auto
  }

  /** The fleet's layer, over black with the sea's whole share (alpha 1) to start, and its glow. */
  private drawLayer(r: THREE.WebGLRenderer, scene: THREE.Scene, camera: THREE.Camera) {
    r.getClearColor(this.clearWas)
    const alpha = r.getClearAlpha()
    r.setClearColor(this.black, 1)
    r.setRenderTarget(this.layer)
    r.clear()
    r.setClearColor(this.clearWas, alpha)
    r.render(scene, camera)
    this.layerStale = false
    const br = this.mats.bright
    br.uniforms.tSrc.value = this.layer.texture
    br.uniforms.uTexel.value.set(1 / this.w, 1 / this.h)
    br.uniforms.uThreshold.value = this.threshold
    this.pass(r, br, this.a1)
    const bl = this.mats.blur
    bl.uniforms.tSrc.value = this.a1.texture
    bl.uniforms.uDir.value.set(1 / this.a1.width, 0)
    this.pass(r, bl, this.a2)
    bl.uniforms.tSrc.value = this.a2.texture
    bl.uniforms.uDir.value.set(0, 1 / this.a1.height)
    this.pass(r, bl, this.a1)
    const dn = this.mats.down
    dn.uniforms.tSrc.value = this.a1.texture
    dn.uniforms.uTexel.value.set(1 / this.a1.width, 1 / this.a1.height)
    this.pass(r, dn, this.b1)
    bl.uniforms.tSrc.value = this.b1.texture
    bl.uniforms.uDir.value.set(1 / this.b1.width, 0)
    this.pass(r, bl, this.b2)
    bl.uniforms.tSrc.value = this.b2.texture
    bl.uniforms.uDir.value.set(0, 1 / this.b1.height)
    this.pass(r, bl, this.b1)
  }

  dispose() {
    for (const t of [this.sea, this.layer, this.a1, this.a2, this.b1, this.b2]) t.dispose()
    for (const m of Object.values(this.mats)) m.dispose()
    this.quad.geometry.dispose()
  }
}
