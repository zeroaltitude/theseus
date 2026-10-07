// The Ship's shaders (theseus-logs). GLSL in three's ShaderMaterial dialect (three rewrites `attribute`, `varying`,
// `texture2D`, and `gl_FragColor` for WebGL2). Every moving thing reads its vessel's transform from one float
// texture, so a vessel that moves to a new slot moves its lights, oars, sails, and wake with one texel each.
//
// The vessel texture: width = capacity, 4 rows.
//   row 0: x, z, heading, length
//   row 1: beam, rig (0 anchor, 1 sail, 2 lantern, 3 flare), flags, born (s)
//   row 2: flare burst (s), streaming (0/1), gold share of planks, seed
//   row 3: unused (the time machine's seam)
// Flags: 1 holds external text, 2 selected, 4 hovered, 8 a task, 16 dimmed by a filter.

export const VESSEL_COMMON = /* glsl */ `
uniform sampler2D uVessels;
uniform float uVesselW;
uniform float uTime;
uniform float uCalm;
vec4 vRow(float i, float row) { return texture2D(uVessels, vec2((i + 0.5) / uVesselW, (row + 0.5) / 4.0)); }
vec3 toWorld(vec4 a, vec3 l) {
  float c = cos(a.z); float s = sin(a.z);
  return vec3(a.x + l.x * c - l.z * s, l.y, a.y + l.x * s + l.z * c);
}
float bit(float flags, float b) { return mod(floor(flags / b + 0.001), 2.0); }
`

const HASH = /* glsl */ `
float hash12(vec2 p) { vec3 p3 = fract(vec3(p.xyx) * 0.1031); p3 += dot(p3, p3.yzx + 33.33); return fract((p3.x + p3.y) * p3.z); }
float hash13(vec3 p3) { p3 = fract(p3 * 0.1031); p3 += dot(p3, p3.zyx + 31.32); return fract((p3.x + p3.y) * p3.z); }
`

// ---------------------------------------------------------------- the sea

export const SEA_VERT = /* glsl */ `
varying vec3 vWorld;
void main() {
  vec4 w = modelMatrix * vec4(position, 1.0);
  vWorld = w.xyz;
  gl_Position = projectionMatrix * viewMatrix * w;
}
`

// A line every unit of a continuous coordinate, anti-aliased by that coordinate's own derivative (never the derivative
// of a fract(), which spikes where it wraps and draws false lines).
const LINE_AT = /* glsl */ `
float lineAt(float coord, float fw, float w) {
  float d = abs(fract(coord - 0.5) - 0.5);
  return 1.0 - smoothstep(w * fw, (w + 1.2) * fw, d);
}
`

// The sea's still parts, drawn into the post's cache when the camera moves: the light table, the chart grid, and the
// compass rose. Its waves and glints are the swell's (SEA_SWELL), drawn over this in the composite.
export const SEA_FRAG = /* glsl */ `
uniform vec2 uTarget;
uniform float uDist;
uniform vec2 uCenter;
uniform float uRose;
varying vec3 vWorld;
${LINE_AT}
float gridAt(vec2 p, float s, float w) {
  vec2 c = p / s;
  vec2 fw = fwidth(c);
  return max(lineAt(c.x, fw.x, w), lineAt(c.y, fw.y, w));
}
void main() {
  vec2 p = vWorld.xz;
  float d = length(p - uTarget) / max(uDist, 1.0);
  float near = smoothstep(2.6, 0.15, d);
  // Night navy, a little lighter under the camera: the hologram's light table.
  vec3 col = mix(vec3(0.012, 0.028, 0.052), vec3(0.030, 0.072, 0.118), near * 0.75);
  // The chart grid: its spacing follows the zoom, so it never turns to mush.
  float lod = log2(max(uDist, 2.0) / 10.0);
  float s1 = 6.0 * exp2(floor(lod));
  float t = fract(lod);
  float minor = gridAt(p, s1, 0.5) * (1.0 - t);
  float major = gridAt(p, s1 * 4.0, 0.7);
  vec3 cyan = vec3(0.13, 0.83, 0.93);
  col += cyan * (minor * 0.045 + major * 0.085) * near;
  // An engraved compass rose under the fleet: thirty-two rays and two rings, in old gold.
  vec2 q = p - uCenter;
  float r = length(q);
  if (uRose > 0.0 && r < uRose * 1.08 && r > 1e-3) {
    // The angle twice, with its seam on opposite sides: the derivative comes from whichever has no seam here.
    float a1 = atan(q.y, q.x) / 6.2831853;
    float a2 = atan(-q.y, -q.x) / 6.2831853;
    float k1 = a1 * 32.0;
    float fwk = min(fwidth(k1), fwidth(a2 * 32.0));
    // Rays: lines at whole k, about a pixel wide; the cardinal four longer and brighter.
    float rayW = 0.8;
    float cardinal = 1.0 - step(0.06, abs(fract(k1 / 8.0 + 0.5) - 0.5) * 8.0);
    float rays = lineAt(k1, fwk, mix(0.45, 0.95, cardinal) * rayW) * step(r, uRose * mix(0.72, 1.0, cardinal)) * smoothstep(uRose * 0.18, uRose * 0.3, r);
    float fr = fwidth(r);
    float rings = (1.0 - smoothstep(0.8 * fr, 2.0 * fr, abs(r - uRose))) + (1.0 - smoothstep(0.5 * fr, 1.7 * fr, abs(r - uRose * 0.94))) + (1.0 - smoothstep(0.5 * fr, 1.7 * fr, abs(r - uRose * 0.3)));
    float kt = a1 * 128.0;
    float ticks = lineAt(kt, min(fwidth(kt), fwidth(a2 * 128.0)), 0.4) * step(uRose * 0.94, r) * step(r, uRose);
    col += vec3(0.84, 0.65, 0.28) * (rays * 0.045 + rings * 0.07 + ticks * 0.035);
  }
  gl_FragColor = vec4(col, 1.0);
}
`

// The swell (theseus-wp2d): the sea's waves and the stars' glints on the water, as light added at a point of the sea.
// The post's composite draws it over the sea's cache, finding each pixel's point of the sea from the camera (no
// texture read), so a frame of the swell alone is one pass. A still swell (Calm, ?swell=0) is added into the sea's
// cache once instead.
//
// - The logo's waves: rows of sine lines, faint teal, alternate rows half a wave apart (one sine, no seams), a few rows
//   to the screen at every zoom. In Live mode they roll: the crests drift along the rows, one passing every 14 s; a
//   slower harmonic runs the other way (31 s) and the rows rise and fall out of step (19 s), so the swell never looks
//   mechanical; and a row's light grows a little as the swell lifts it.
// - A faint sparkle on the water, stars reflected from the logo's night, glinting each at its own pace (4 to 9 s).
// - uSwell is the swell's clock in seconds. It runs only in Live mode, and the swell comes up over its first seconds,
//   so at 0 the sea is the still one (Calm from the start, and ?swell=0).
// - It runs on every pixel of every frame of the swell, so what the whole screen shares is worked out once, in the
//   engine: the rows' scale (uWaveScale) and the fade's (uNearScale). Three sines a pixel, and a fourth in a glint's
//   cell: each costs on a CPU rasteriser.
export const SEA_SWELL = /* glsl */ `
uniform vec2 uTarget;
uniform float uNearScale;
uniform float uWaveScale;
uniform float uSwell;
uniform float uWaves;
uniform vec3 uCamPos;
uniform vec3 uRayC;
uniform vec3 uRayX;
uniform vec3 uRayY;
${LINE_AT}
float seaHash(vec2 p) { vec3 p3 = fract(vec3(p.xyx) * 0.1031); p3 += dot(p3, p3.yzx + 33.33); return fract((p3.x + p3.y) * p3.z); }
vec3 seaSwell(vec2 uv) {
  // The point of the sea under this pixel: the camera's ray through it, to the water (y = 0). Every pixel sees the
  // water at the Ship's pitches; the clamp only keeps the division safe.
  vec3 dir = uRayC + (uv.x * 2.0 - 1.0) * uRayX + (uv.y * 2.0 - 1.0) * uRayY;
  vec2 p = uCamPos.xz + dir.xz * (uCamPos.y / max(-dir.y, 1e-5));
  float near = smoothstep(2.6, 0.15, length(p - uTarget) * uNearScale);
  vec2 sp = p * uWaveScale;
  float rise = smoothstep(0.0, 6.0, uSwell);
  float lift = sin(sp.y * 0.6 - uSwell * 0.3307);
  float wy = sp.y + sin(sp.x * 4.2 + sp.y * 3.1415927 - uSwell * 0.4488) * 0.11
    + rise * (sin(sp.x * 1.7 + sp.y * 0.9 + uSwell * 0.2027) * 0.04 + lift * 0.05);
  float wave = lineAt(wy, fwidth(wy), 0.6);
  vec3 col = vec3(0.24, 0.49, 0.58) * wave * 0.16 * (1.0 + rise * 0.14 * lift) * near;
  // The sparkle shows only once a cell is a few pixels across (smaller, a dot would spill past its cell and be cut
  // into a dash).
  vec2 cell = floor(p / 9.0);
  float fwp = max(fwidth(p.x), fwidth(p.y));
  float cellPx = 9.0 / max(fwp, 1e-4);
  if (seaHash(cell) > 0.93 && cellPx > 8.0) {
    vec2 c = (cell + 0.2 + 0.6 * vec2(seaHash(cell + 7.1), seaHash(cell + 3.3))) * 9.0;
    float sd = length(p - c);
    float rad = min(0.18 + fwp * 1.2, 1.6);
    float glint = 1.0 + rise * 0.3 * sin(uSwell * (0.7 + seaHash(cell + 5.9) * 0.86) + seaHash(cell + 1.7) * 6.2831853);
    col += vec3(0.91, 0.79, 0.50) * (1.0 - smoothstep(rad * 0.5, rad, sd)) * 0.5 * near * smoothstep(8.0, 18.0, cellPx) * glint;
  }
  return col * uWaves;
}
`

// ---------------------------------------------------------------- the stars (deep, under the sea, with parallax)

export const STAR_VERT = /* glsl */ `
uniform vec2 uTarget;
uniform float uScale;
attribute float aSize;
attribute float aTone;
varying float vTone;
void main() {
  // Stars sit far below the chart: they drift at a third of the sea's pace.
  vec3 w = position + vec3(uTarget.x * 0.62, 0.0, uTarget.y * 0.62);
  vec4 mv = viewMatrix * vec4(w, 1.0);
  gl_Position = projectionMatrix * mv;
  gl_PointSize = clamp(aSize * uScale / -mv.z, 1.0, 3.5);
  vTone = aTone;
}
`

export const STAR_FRAG = /* glsl */ `
varying float vTone;
void main() {
  vec2 uv = gl_PointCoord * 2.0 - 1.0;
  float a = smoothstep(1.0, 0.0, length(uv));
  gl_FragColor = vec4(vec3(0.91, 0.79, 0.50) * vTone * a, a * vTone);
}
`

// ---------------------------------------------------------------- hulls

export const HULL_VERT = /* glsl */ `
${VESSEL_COMMON}
attribute float aIdx;
varying vec2 vLocal;
varying float vL;
varying float vB;
varying float vRig;
varying float vFlags;
varying float vBorn;
varying vec4 vC;
void main() {
  vec4 a = vRow(aIdx, 0.0);
  vec4 b = vRow(aIdx, 1.0);
  vC = vRow(aIdx, 2.0);
  vL = a.w; vB = b.x; vRig = b.y; vFlags = b.z; vBorn = b.w;
  float pad = 2.2 + vB * 0.7;
  vec3 l = vec3(position.x * (vL + pad * 2.0), 0.0, position.z * (vB + pad * 2.0));
  vLocal = l.xz;
  gl_Position = projectionMatrix * viewMatrix * vec4(toWorld(a, l), 1.0);
}
`

export const HULL_FRAG = /* glsl */ `
${VESSEL_COMMON}
${HASH}
varying vec2 vLocal;
varying float vL;
varying float vB;
varying float vRig;
varying float vFlags;
varying float vBorn;
varying vec4 vC;
// The galley seen from above: a long taper to a ram at the bow, a full waist aft of midships, and a stern that
// narrows to the curl of its sternpost.
float halfWidth(float t) {
  if (t > 0.05) { float k = (t - 0.05) / 0.95; return max(0.0, pow(1.0 - pow(k, 1.35), 0.85)); }
  if (t < -0.42) { float k = (-0.42 - t) / 0.58; return max(0.0, sqrt(max(0.0, 1.0 - k * k)) * (1.0 - 0.55 * k * k) ); }
  return 1.0;
}
vec3 rigColor(float rig) {
  if (rig > 2.5) return vec3(1.0, 0.45, 0.53);          // flare: rose
  if (rig > 1.5) return vec3(1.0, 0.77, 0.15);          // lantern: amber
  if (rig > 0.5) return vec3(0.14, 0.89, 1.0);          // under sail: neon cyan
  return vec3(0.69, 0.55, 0.34) * 0.62;                 // at anchor: brass, quiet
}
void main() {
  float t = vLocal.x / (vL * 0.5);
  float hw = halfWidth(clamp(t, -1.0, 1.0)) * vB * 0.5;
  float sd = max(abs(vLocal.y) - hw, abs(vLocal.x) - vL * 0.5);
  float aa = fwidth(sd) + 1e-4;
  float inside = 1.0 - smoothstep(-aa, aa, sd);
  // The rail: a crisp line about two pixels wide at any zoom, and a soft glow outside it.
  float edge = 1.0 - smoothstep(aa * 1.1, aa * 2.4, abs(sd));
  float glow = exp(-max(sd, 0.0) / max(aa * 9.0, 0.02)) * 0.55 + exp(-max(sd, 0.0) * 3.0 / max(vB * 0.3, 0.5)) * 0.25;
  glow *= 1.0 - inside;
  vec3 ec = rigColor(vRig);
  float hovered = bit(vFlags, 4.0);
  float selected = bit(vFlags, 2.0);
  float dim = bit(vFlags, 16.0);
  ec *= 1.0 + hovered * 0.5;
  // A new vessel (a session that opened while you watched) flares in.
  float age = uTime - vBorn;
  float born = vBorn > 0.0 ? exp(-max(age, 0.0) * 1.4) * (1.0 - uCalm * 0.6) : 0.0;
  ec = mix(ec, vec3(1.0, 0.9, 0.6), clamp(born, 0.0, 1.0));

  // The deck's planks: five strakes, butts staggered like brickwork; the share of gold ones is the share of its
  // turns in the last hour, the new planks of the ship.
  float nb = 5.0;
  float across = clamp(vLocal.y / max(hw, 1e-3) * 0.5 + 0.5, 0.0, 0.9999);
  float band = floor(across * nb);
  float segLen = max(vL / 7.0, 1.6);
  float off = mod(band, 2.0) * 0.5 * segLen;
  float along = (vLocal.x + vL + off) / segLen;
  float seg = floor(along);
  float gold = step(hash13(vec3(band, seg, vC.w)), vC.z);
  // The seams between planks: a line where along (or the strake index) crosses a whole number.
  float fa = fwidth(along);
  float jointX = 1.0 - smoothstep(fa * 0.6, fa * 1.6, abs(fract(along + 0.5) - 0.5));
  float bandF = across * nb;
  float fb = fwidth(bandF);
  float jointZ = 1.0 - smoothstep(fb * 0.6, fb * 1.6, abs(fract(bandF + 0.5) - 0.5));
  float joint = max(jointX * 0.9, jointZ);
  vec3 ivory = vec3(0.94, 0.89, 0.78);
  vec3 goldC = vec3(0.89, 0.68, 0.31);
  vec3 plank = mix(ivory * 0.30, goldC * 0.52, gold) * (1.0 - joint * 0.8);
  float plankA = mix(0.34, 0.5, gold) * (1.0 - joint * 0.35);

  vec3 col = plank * inside;
  float alpha = plankA * inside;

  // The eye at the bow, as on the coin.
  vec2 eye = vec2(vL * 0.5 - max(vB * 0.75, vL * 0.1), sign(vLocal.y) * vB * 0.2);
  vec2 e = (vLocal - eye) / vec2(max(vB * 0.22, 0.3), max(vB * 0.09, 0.12));
  float eyeA = (1.0 - smoothstep(0.85, 1.0, length(e))) * inside;
  float pupil = (1.0 - smoothstep(0.35, 0.5, length(e * vec2(1.0, 0.5)))) * eyeA;
  col = mix(col, ivory * 0.42, eyeA * 0.75);
  col = mix(col, goldC * 0.9, pupil);
  alpha = max(alpha, eyeA * 0.7);

  // The latch: a session that read external text carries a chain along its rail until the operator trusts it.
  if (bit(vFlags, 1.0) > 0.5) {
    float rail = -sd;
    float band2 = 1.0 - smoothstep(0.08, 0.16, abs(rail - vB * 0.09 - 0.12));
    float linkP = vLocal.x / max(vB * 0.16, 0.28);
    float link = abs(fract(linkP) - 0.5);
    float ringL = 1.0 - smoothstep(0.06, 0.16, abs(length(vec2(link * 1.6, (rail - vB * 0.09 - 0.12) / 0.12)) - 0.55));
    float chain = band2 * ringL * inside;
    vec3 cc = mix(vec3(1.0, 0.77, 0.15), vec3(1.0, 0.47, 0.74), 0.45);
    col += cc * chain;
    alpha = max(alpha, chain);
  }

  float hot = vRig > 0.5 ? 1.0 : 0.45;
  // Far out, a hull is a few dozen pixels: its glow would swallow its shape, so the glow comes up as the hull grows on
  // screen (theseus-hnof), and the rail's colour carries the state.
  float hullPx = vL / max(aa, 1e-4);
  float near = smoothstep(70.0, 260.0, hullPx);
  hot *= mix(0.28, 1.0, near);
  // A small hull's bright rail (cyan, amber, rose) would bloom into a blob: it is dimmed until the hull is big enough to
  // keep its outline, and the nameplate says the state in words.
  ec *= mix(vRig > 0.5 ? 0.42 : 1.0, 1.0, near);
  col += ec * edge * 1.15 + ec * glow * 0.42 * hot;
  alpha = max(alpha, edge * 0.95 + glow * 0.55 * hot);

  // Selected: a dashed range ring, neon cyan.
  // Selected: a dashed ellipse around it, a range-finder's lock, in neon cyan.
  if (selected > 0.5) {
    vec2 radii = vec2(vL * 0.5 + vB * 0.6 + 0.8, vB * 1.15 + 0.9);
    float e = length(vLocal / radii);
    float fe = fwidth(e);
    float ring = 1.0 - smoothstep(fe * 0.7, fe * 1.9, abs(e - 1.0));
    float ang = atan(vLocal.y / radii.y, vLocal.x / radii.x);
    float dash = step(0.38, fract(ang / 6.2831853 * 48.0));
    col += vec3(0.14, 0.89, 1.0) * ring * dash;
    alpha = max(alpha, ring * dash * 0.9);
  }
  col *= 1.0 - dim * 0.7;
  alpha *= 1.0 - dim * 0.6;
  if (alpha < 0.004) discard;
  gl_FragColor = vec4(col, alpha);
}
`

// ---------------------------------------------------------------- sails (only under sail)

export const SAIL_VERT = /* glsl */ `
${VESSEL_COMMON}
uniform float uScale;
attribute float aIdx;
varying vec2 vUv;
varying float vBorn;
varying float vFade;
void main() {
  vec4 a = vRow(aIdx, 0.0);
  vec4 b = vRow(aIdx, 1.0);
  vUv = position.xy + 0.5;
  vBorn = b.w;
  if (b.y < 0.5 || b.y > 1.5) { gl_Position = vec4(2.0, 2.0, 2.0, 1.0); vFade = 0.0; return; }
  // The sail stands forward of midships and smaller than the hull is long, so the benches aft stay in view.
  vec3 foot = toWorld(a, vec3(a.w * 0.2, 0.25, 0.0));
  // A sail shows once its hull is big enough on screen to carry one (far out, the cyan rail says it), and folds away
  // again close in, at a turn's depth, where the benches and oars are the subject and the rail still says the state.
  float depth = max(1.0, -(viewMatrix * vec4(foot, 1.0)).z);
  float px = a.w * uScale / depth;
  // The hull's length against the height the camera sees there: under 1 at a ship's depth, about 2 at a turn's.
  float rel = a.w * projectionMatrix[1][1] / (2.0 * depth);
  vFade = smoothstep(90.0, 170.0, px) * (1.0 - smoothstep(1.3, 1.9, rel));
  vec3 right = vec3(viewMatrix[0][0], viewMatrix[1][0], viewMatrix[2][0]);
  vec3 up = vec3(viewMatrix[0][1], viewMatrix[1][1], viewMatrix[2][1]);
  float w = a.w * 0.17 + 0.4;
  float h = a.w * 0.2 + 0.5;
  vec3 p = foot + right * (position.x * w) + up * ((position.y + 0.5) * h);
  gl_Position = projectionMatrix * viewMatrix * vec4(p, 1.0);
}
`

export const SAIL_FRAG = /* glsl */ `
varying vec2 vUv;
varying float vFade;
float lineAA(float d, float w) { float f = fwidth(d); return 1.0 - smoothstep(w * f, (w + 1.2) * f, abs(d)); }
void main() {
  vec2 p = vUv - vec2(0.5, 0.0);
  // The sail hangs from the yard (top), its foot bellied (the logo's sail).
  float top = 0.92;
  float foot = 0.18 + 0.06 * (1.0 - pow(abs(p.x) * 2.0, 2.0));
  float halfW = mix(0.36, 0.42, (p.y - foot) / (top - foot));
  float inSail = step(foot, p.y) * step(p.y, top) * step(abs(p.x), halfW);
  float dEdge = min(min(p.y - foot, top - p.y), halfW - abs(p.x));
  float edge = inSail * (1.0 - smoothstep(0.0, fwidth(dEdge) * 2.0 + 0.012, dEdge));
  float mast = lineAA(p.x, 1.2) * step(0.02, p.y) * step(p.y, 0.99);
  float yard = lineAA(p.y - 0.95, 1.4) * step(abs(p.x), 0.48);
  // The theta, as on the coin.
  vec2 c = (p - vec2(0.0, 0.56)) / vec2(0.14, 0.17);
  float th = lineAA(length(c) - 1.0, 1.6) + lineAA(c.y, 1.4) * step(abs(c.x), 0.55);
  vec3 ivory = vec3(0.94, 0.89, 0.78);
  vec3 cyan = vec3(0.13, 0.83, 0.93);
  vec3 col = ivory * 0.13 * inSail + cyan * edge + ivory * 0.75 * (mast + yard);
  col = mix(col, cyan * 0.9, clamp(th, 0.0, 1.0) * inSail);
  float alpha = max(inSail * 0.2, max(edge * 0.95, max(mast, yard) * 0.8));
  alpha = max(alpha, clamp(th, 0.0, 1.0) * inSail * 0.9);
  alpha *= vFade;
  if (alpha < 0.01) discard;
  gl_FragColor = vec4(col, alpha);
}
`

// ---------------------------------------------------------------- lights (every node)

export const LIGHT_VERT = /* glsl */ `
${VESSEL_COMMON}
uniform float uScale;
uniform float uPixel;
attribute float aIdx;
attribute vec3 aColor;
attribute float aSize;
attribute float aFlags;
attribute vec2 aTimes;
varying vec3 vColor;
varying float vFlags;
varying vec2 vAge;
varying float vRing;
varying float vFade;
void main() {
  vec4 a = vRow(aIdx, 0.0);
  vec4 b = vRow(aIdx, 1.0);
  vec3 w = toWorld(a, position);
  vec4 mv = viewMatrix * vec4(w, 1.0);
  gl_Position = projectionMatrix * mv;
  float age = aTimes.x > 0.0 ? uTime - aTimes.x : 1e6;
  float flare = exp(-max(age, 0.0) * 2.2) * step(0.0, age);
  // Rings (the L1 shield, external text, a running gear) need room around the core.
  float ringed = max(max(bit(aFlags, 1.0), bit(aFlags, 2.0)), bit(aFlags, 4.0));
  float s = aSize * (1.0 + ringed * 1.3) * (1.0 + flare * 2.6 * (1.0 - uCalm * 0.7)) * (1.0 + bit(aFlags, 16.0) * 1.4);
  float dim = max(bit(b.z, 16.0), bit(aFlags, 32.0));
  float px = s * uScale / -mv.z;
  gl_PointSize = clamp(px, 2.2 * uPixel, 72.0 * uPixel);
  // A light smaller than the smallest point we draw gives only its share of the light: far out, a keel of fifty
  // lights reads as a faint string, not a white blob.
  vFade = clamp(px / (2.2 * uPixel), 0.18, 1.0);
  // And the keel's lights come up as their vessel grows on screen: far out, its rail and rig say enough.
  float hullPx = a.w * uScale / max(1.0, -(viewMatrix * vec4(a.x, 0.0, a.y, 1.0)).z);
  vFade *= mix(0.22, 1.0, smoothstep(40.0 * uPixel, 180.0 * uPixel, hullPx));
  vColor = aColor * (1.0 - dim * 0.75);
  vFlags = aFlags;
  if (aSize <= 0.0) { gl_Position = vec4(2.0, 2.0, 2.0, 1.0); gl_PointSize = 0.0; }
  // y: a verified cancel's time (the shield collapses over it); below zero, collapsed before the page loaded.
  vAge = vec2(age, aTimes.y > 0.0 ? uTime - aTimes.y : aTimes.y < 0.0 ? 1e6 : -1.0);
  vRing = ringed;
}
`

export const LIGHT_FRAG = /* glsl */ `
uniform float uTime;
uniform float uCalm;
varying vec3 vColor;
varying float vFlags;
varying vec2 vAge;
varying float vRing;
varying float vFade;
float bit(float flags, float b) { return mod(floor(flags / b + 0.001), 2.0); }
float hexDist(vec2 p) { p = abs(p); return max(p.x * 0.8660254 + p.y * 0.5, p.y); }
void main() {
  vec2 uv = gl_PointCoord * 2.0 - 1.0;
  float r = length(uv);
  float k = mix(1.0, 2.3, vRing);           // the core shrinks inside a ring
  float rc = r * k;
  float core = 1.0 - smoothstep(0.18, 0.42, rc);
  float halo = exp(-rc * rc * 5.0) * 0.7;
  vec3 col = vColor * min(1.0, core + halo);
  float alpha = core + halo * 0.8;
  // L1: a hexagonal shield, verdigris neon. A cancel verified collapses it.
  if (bit(vFlags, 1.0) > 0.5) {
    float shrink = vAge.y >= 0.0 ? clamp(1.0 - vAge.y / 1.2, 0.0, 1.0) : 1.0;
    float hd = hexDist(uv) - 0.78 * shrink;
    float ring = 1.0 - smoothstep(0.03, 0.09, abs(hd));
    float fill = (1.0 - smoothstep(0.0, 0.04, hd)) * 0.10;
    vec3 sc = vec3(0.42, 1.0, 0.88);
    col += sc * (ring + fill) * shrink;
    alpha = max(alpha, (ring + fill) * shrink);
    // What a collapse leaves: a small rose hex, so a stopped job still reads as one.
    if (vAge.y >= 0.0) {
      float gone = 1.0 - shrink;
      float ember = 1.0 - smoothstep(0.02, 0.06, abs(hexDist(uv) - 0.36));
      col += vec3(1.0, 0.45, 0.52) * ember * gone * 0.85;
      alpha = max(alpha, ember * gone * 0.85);
    }
  }
  // External text: a warning ring in magenta.
  if (bit(vFlags, 2.0) > 0.5) {
    float ring = 1.0 - smoothstep(0.04, 0.11, abs(r - 0.66));
    vec3 mc = vec3(1.0, 0.47, 0.74);
    col += mc * ring + mc * 0.3 * exp(-r * r * 2.5);
    alpha = max(alpha, ring);
  }
  // A job running now: a turning gear.
  if (bit(vFlags, 4.0) > 0.5) {
    float ang = atan(uv.y, uv.x) + uTime * 1.6 * (1.0 - uCalm);
    float teeth = step(0.5, fract(ang / 6.2831853 * 10.0));
    float rr = 0.80 + teeth * 0.1;
    float gear = 1.0 - smoothstep(0.03, 0.08, abs(r - rr + 0.05));
    vec3 gc = vec3(1.0, 0.8, 0.36);
    col += gc * gear;
    alpha = max(alpha, gear);
  }
  // Highlighted (flown to, or the inspector's): a bright cross-hair ring.
  if (bit(vFlags, 16.0) > 0.5) {
    float ring = 1.0 - smoothstep(0.02, 0.06, abs(r - 0.92));
    col += vec3(1.0, 0.96, 0.88) * ring;
    alpha = max(alpha, ring);
  }
  // A fact arrived: a flare, then a ring that runs out and fades.
  if (vAge.x < 2.0) {
    float ring = 1.0 - smoothstep(0.0, 0.08, abs(r - mix(0.2, 0.98, clamp(vAge.x / 1.1, 0.0, 1.0))));
    float f = exp(-vAge.x * 2.0);
    col += vec3(1.0, 0.93, 0.74) * (ring * f + core * f);
    alpha = max(alpha, ring * f);
  }
  col *= vFade;
  alpha *= vFade;
  if (alpha < 0.01) discard;
  gl_FragColor = vec4(col, clamp(alpha, 0.0, 1.0));
}
`

// ---------------------------------------------------------------- lines (keels, oars, formation rings)

export const LINE_VERT = /* glsl */ `
${VESSEL_COMMON}
uniform float uScale;
uniform float uPixel;
attribute float aIdx;
attribute vec4 aColor;
varying vec4 vColor;
void main() {
  vec3 w = position;
  float dim = 0.0;
  float fade = 1.0;
  if (aIdx >= 0.0) {
    vec4 a = vRow(aIdx, 0.0);
    dim = bit(vRow(aIdx, 1.0).z, 16.0);
    w = toWorld(a, position);
    // Oars and keels come up as their vessel grows on screen.
    float hullPx = a.w * uScale / max(1.0, -(viewMatrix * vec4(a.x, 0.0, a.y, 1.0)).z);
    fade = mix(0.15, 1.0, smoothstep(50.0 * uPixel, 200.0 * uPixel, hullPx));
  }
  vColor = vec4(aColor.rgb * (1.0 - dim * 0.7) * fade, aColor.a);
  gl_Position = projectionMatrix * viewMatrix * vec4(w, 1.0);
}
`

export const LINE_FRAG = /* glsl */ `
varying vec4 vColor;
void main() { gl_FragColor = vColor; }
`

// ---------------------------------------------------------------- currents and tethers (particles along a curve)

export const FLOW_VERT = /* glsl */ `
${VESSEL_COMMON}
uniform float uScale;
uniform float uPixel;
attribute vec3 aFrom;    // vessel, local x, local z
attribute vec3 aTo;
attribute vec4 aParam;   // t, speed, sag, burst time
attribute vec3 aColor;
attribute float aSize;
varying vec3 vColor;
varying float vA;
void main() {
  vec3 p0 = toWorld(vRow(aFrom.x, 0.0), vec3(aFrom.y, 0.05, aFrom.z));
  vec3 p2 = toWorld(vRow(aTo.x, 0.0), vec3(aTo.y, 0.05, aTo.z));
  vec3 d = p2 - p0;
  vec3 n = normalize(vec3(-d.z, 0.0, d.x) + 1e-5);
  vec3 p1 = (p0 + p2) * 0.5 + n * aParam.z * length(d) + vec3(0.0, length(d) * 0.04, 0.0);
  float burst = aParam.w > 0.0 ? uTime - aParam.w : 1e6;
  float t = aParam.x;
  vec3 col = aColor;
  float a = 0.55;
  float grow = 1.0;
  if (burst < 3.0) {
    // The report lands: a run of gold from the task back to its parent.
    t = 1.0 - fract(aParam.x + burst * 0.9 * (1.0 - uCalm));
    col = vec3(1.0, 0.8, 0.38);
    a = exp(-burst * 0.45);
    grow = 2.4;
  } else if (aParam.y > 0.0) {
    t = fract(aParam.x + uTime * aParam.y * (1.0 - uCalm));
    a = 0.75;
  }
  float u = 1.0 - t;
  vec3 p = u * u * p0 + 2.0 * u * t * p1 + t * t * p2;
  vec4 mv = viewMatrix * vec4(p, 1.0);
  gl_Position = projectionMatrix * mv;
  gl_PointSize = clamp(aSize * grow * uScale / -mv.z, 1.8 * uPixel * grow, 9.0 * uPixel * grow);
  vColor = col;
  vA = a * smoothstep(0.0, 0.08, t) * smoothstep(1.0, 0.92, t);
}
`

export const FLOW_FRAG = /* glsl */ `
varying vec3 vColor;
varying float vA;
void main() {
  vec2 uv = gl_PointCoord * 2.0 - 1.0;
  float a = exp(-dot(uv, uv) * 3.0) * vA;
  if (a < 0.01) discard;
  gl_FragColor = vec4(vColor * a, a);
}
`

// ---------------------------------------------------------------- wakes (behind a vessel under sail)

export const WAKE_VERT = /* glsl */ `
${VESSEL_COMMON}
uniform float uScale;
uniform float uPixel;
attribute float aIdx;
attribute vec2 aSeed;    // phase, side
varying float vA;
void main() {
  vec4 a = vRow(aIdx, 0.0);
  vec4 b = vRow(aIdx, 1.0);
  if (b.y < 0.5 || b.y > 1.5) { gl_Position = vec4(2.0, 2.0, 2.0, 1.0); vA = 0.0; return; }
  float age = fract(aSeed.x + uTime * 0.32 * (1.0 - uCalm));
  float L = a.w; float B = b.x;
  vec3 l = vec3(-L * 0.5 - age * L * 0.85, 0.03, aSeed.y * (B * 0.22 + age * B * 1.25));
  vec4 mv = viewMatrix * vec4(toWorld(a, l), 1.0);
  gl_Position = projectionMatrix * mv;
  gl_PointSize = clamp((0.5 + age * 0.9) * uScale / -mv.z, 1.4 * uPixel, 16.0 * uPixel);
  vA = pow(1.0 - age, 1.6) * 0.7;
  // A wake shows once its hull is big enough to have one (theseus-hnof): far out it only blurs the hull.
  float hullPx = L * uScale / max(1.0, -(viewMatrix * vec4(a.x, 0.0, a.y, 1.0)).z);
  vA *= smoothstep(60.0 * uPixel, 200.0 * uPixel, hullPx);
}
`

export const WAKE_FRAG = /* glsl */ `
varying float vA;
void main() {
  vec2 uv = gl_PointCoord * 2.0 - 1.0;
  float a = exp(-dot(uv, uv) * 2.6) * vA;
  if (a < 0.01) discard;
  gl_FragColor = vec4(vec3(0.62, 0.90, 0.95) * a, a);
}
`

// ---------------------------------------------------------------- beacons (lantern, flare, a model streaming)

export const BEACON_VERT = /* glsl */ `
${VESSEL_COMMON}
uniform float uScale;
uniform float uPixel;
attribute float aIdx;
attribute float aKind;   // 0 the rig's beacon, 1 a model call streaming, 2 a failure's burst
attribute vec3 aAt;      // where the next light will land (for the stream)
varying vec3 vColor;
varying float vA;
void main() {
  vec4 a = vRow(aIdx, 0.0);
  vec4 b = vRow(aIdx, 1.0);
  vec4 c = vRow(aIdx, 2.0);
  float L = a.w; float B = b.x;
  vec3 l = vec3(0.0);
  vColor = vec3(0.0); vA = 0.0;
  float size = 0.0;
  float pulse = 1.0;
  if (aKind < 0.5) {
    if (b.y > 1.5 && b.y < 2.5) {
      // The lantern: it waits for the operator. It hangs on the stern post, low, so it reads as the ship's own lamp and
      // not a light in the sky (theseus-hnof).
      l = vec3(-L * 0.47, B * 0.45 + 0.35, 0.0);
      vColor = vec3(1.0, 0.77, 0.15);
      pulse = 0.78 + 0.22 * sin(uTime * 2.4) * (1.0 - uCalm);
      size = 1.6 + B * 0.45;
    } else if (b.y > 2.5) {
      // The flare: it failed.
      l = vec3(L * 0.1, B * 1.6 + 1.2, 0.0);
      vColor = vec3(1.0, 0.36, 0.42);
      size = 1.4 + B * 0.22;
    }
  } else if (aKind < 1.5) {
    if (c.y > 0.5) {
      l = aAt;
      vColor = vec3(0.68, 0.58, 1.0);
      pulse = 0.6 + 0.4 * sin(uTime * 7.0) * (1.0 - uCalm);
      size = 1.1;
    }
  } else {
    float age = c.x > 0.0 ? uTime - c.x : 1e6;
    if (age < 2.6) {
      l = vec3(L * 0.1, B * 1.0 + age * B * 1.6, 0.0);
      vColor = vec3(1.0, 0.42, 0.42);
      pulse = exp(-age * 1.2);
      size = 2.0 + age * 1.5;
    }
  }
  if (size <= 0.0) { gl_Position = vec4(2.0, 2.0, 2.0, 1.0); return; }
  vec4 mv = viewMatrix * vec4(toWorld(a, l), 1.0);
  gl_Position = projectionMatrix * mv;
  gl_PointSize = clamp(size * pulse * uScale / -mv.z, 3.0 * uPixel, 90.0 * uPixel);
  vA = pulse;
}
`

export const BEACON_FRAG = /* glsl */ `
varying vec3 vColor;
varying float vA;
void main() {
  vec2 uv = gl_PointCoord * 2.0 - 1.0;
  float r = length(uv);
  float core = 1.0 - smoothstep(0.1, 0.3, r);
  float halo = exp(-r * r * 4.0);
  float a = (core + halo * 0.6) * vA;
  if (a < 0.01) discard;
  gl_FragColor = vec4(vColor * min(1.0, core + halo * 0.7) * vA, a);
}
`

// ---------------------------------------------------------------- oars (every tool call; theseus-hnof)
//
// One instanced quad an oar, from its oarlock (the call, on the hull) to past its blade (the result): a thin shaft and a
// paddle at the end. The blade's colour is the result: green ok, rose failed, magenta when it carried text from the web,
// amber while the call waits for the operator, an open outline while it is pending, and a turning brass gear while its
// job runs. While its turn runs, the bench's oars row (a slow stroke, out of step by side); an oar that arrives grows
// out from the hull. Calm stills both.
// aState: x the call's arrival (s), y the result's arrival (s), z the stroke's phase, w flags:
//   1 its turn runs (it rows), 2 failed, 4 pending, 8 waits for the operator, 16 external text, 32 a job running,
//   64 dimmed by an overlay, 128 highlighted, 256 sandboxed (L1).

export const OAR_VERT = /* glsl */ `
${VESSEL_COMMON}
uniform float uScale;
uniform float uPixel;
attribute float aIdx;
attribute vec4 aEnds;    // pivot x, z; tip x, z (local)
attribute vec3 aColor;
attribute vec4 aState;
varying vec2 vUv;
varying vec3 vColor;
varying float vFlags;
varying float vLen;
varying float vW;
varying float vFade;
varying float vResult;
void main() {
  vec4 a = vRow(aIdx, 0.0);
  vec4 b = vRow(aIdx, 1.0);
  vec2 piv = aEnds.xy;
  vec2 d = aEnds.zw - piv;
  float flags = aState.w;
  // The stroke: a sweep about the oarlock, aft and back, both sides together (mirrored), with the blade dipping.
  float rowing = bit(flags, 1.0) * (1.0 - uCalm);
  float side = sign(d.y + 1e-5);
  float ph = uTime * 2.6 + aState.z;
  float ang = rowing * 0.30 * sin(ph) * side;
  float c = cos(ang); float s = sin(ang);
  d = vec2(d.x * c - d.y * s, d.x * s + d.y * c);
  // An oar that arrives while we watch grows out from the hull.
  float grow = aState.x > 0.0 ? smoothstep(0.0, 1.0, clamp((uTime - aState.x) / 0.6, 0.0, 1.0)) : 1.0;
  grow = mix(grow, 1.0, uCalm);
  d *= max(grow, 0.02);
  float len = length(d);
  vec2 dir = d / max(len, 1e-4);
  vec2 nrm = vec2(-dir.y, dir.x);
  float B = b.x;
  float w = clamp(B * 0.17, 0.3, 0.95);
  float over = w * 0.35;
  // position.xy in [-0.5, 0.5]: x along the oar (pivot to past the tip), y across.
  float t = position.x + 0.5;
  vec2 p = piv + dir * (t * (len + over)) + nrm * (position.y * w);
  float lift = 0.18 + rowing * 0.12 * max(0.0, cos(ph));
  vec3 world = toWorld(a, vec3(p.x, lift, p.y));
  gl_Position = projectionMatrix * viewMatrix * vec4(world, 1.0);
  vUv = vec2(t * (len + over), position.y * w);
  vLen = len;
  vW = w;
  vFlags = flags;
  vColor = aColor;
  vResult = aState.y > 0.0 ? uTime - aState.y : 1e6;
  // Oars come up as their vessel grows on screen: far out, the rail and the rig say enough.
  float hullPx = a.w * uScale / max(1.0, -(viewMatrix * vec4(a.x, 0.0, a.y, 1.0)).z);
  vFade = smoothstep(50.0 * uPixel, 190.0 * uPixel, hullPx);
  float dim = max(bit(b.z, 16.0), bit(flags, 64.0));
  vFade *= 1.0 - dim * 0.78;
}
`

export const OAR_FRAG = /* glsl */ `
uniform float uTime;
uniform float uCalm;
varying vec2 vUv;
varying vec3 vColor;
varying float vFlags;
varying float vLen;
varying float vW;
varying float vFade;
varying float vResult;
float bit(float flags, float b) { return mod(floor(flags / b + 0.001), 2.0); }
void main() {
  float along = vUv.x;
  float across = vUv.y;
  float bladeLen = min(vLen * 0.42, vW * 2.4);
  float bladeStart = vLen + vW * 0.35 - bladeLen;
  // The shaft: a thin brass line from the oarlock to the blade.
  float sw = vW * 0.07;
  float fa = fwidth(across) + 1e-5;
  float shaft = (1.0 - smoothstep(sw - fa, sw + fa, abs(across))) * step(0.0, along) * step(along, bladeStart + vW * 0.1);
  vec3 brass = vec3(0.80, 0.64, 0.38);
  vec3 col = brass * 0.55 * shaft;
  float alpha = shaft * 0.85;
  // The blade: a paddle, rounded at its end.
  vec2 q = vec2((along - bladeStart) / bladeLen, across / (vW * 0.5));
  float inBladeX = step(0.0, q.x);
  float r = length(vec2(max(q.x - 0.62, 0.0) / 0.38, q.y));
  float edgeD = max(1.0 - r, 0.0);
  float bladeSd = r - 1.0;
  float fb = fwidth(bladeSd) + 1e-4;
  float blade = (1.0 - smoothstep(-fb, fb, bladeSd)) * inBladeX;
  float rim = (1.0 - smoothstep(fb * 0.5, fb * 2.2, abs(bladeSd))) * inBladeX;
  float pending = bit(vFlags, 4.0);
  float waiting = bit(vFlags, 8.0);
  float job = bit(vFlags, 32.0);
  vec3 c = vColor;
  float fill = pending > 0.5 ? 0.0 : 0.5;
  if (waiting > 0.5) fill = 0.42 + 0.3 * (0.5 + 0.5 * sin(uTime * 2.4)) * (1.0 - uCalm);
  if (job > 0.5) {
    // A job running: the blade is a brass gear, turning.
    vec2 g = vec2((along - (bladeStart + bladeLen * 0.5)) / (bladeLen * 0.5), across / (vW * 0.5));
    float gr = length(g);
    float ga = atan(g.y, g.x) + uTime * 1.6 * (1.0 - uCalm);
    float teeth = step(0.5, fract(ga / 6.2831853 * 9.0));
    float gear = 1.0 - smoothstep(0.04, 0.12, abs(gr - (0.72 + teeth * 0.16)));
    float hub = 1.0 - smoothstep(0.18, 0.26, gr);
    vec3 gc = vec3(1.0, 0.8, 0.36);
    col += gc * (gear + hub * 0.6);
    alpha = max(alpha, max(gear, hub * 0.8));
  } else {
    col += c * (blade * fill + rim * 1.05);
    alpha = max(alpha, blade * max(fill, 0.08) + rim * 0.95);
    // A failed result: a cross on the blade.
    if (bit(vFlags, 2.0) > 0.5) {
      vec2 xq = vec2((q.x - 0.55) * bladeLen, q.y * vW * 0.5) / (vW * 0.32);
      float xd = min(abs(xq.x - xq.y), abs(xq.x + xq.y)) * 0.7071;
      float cross = (1.0 - smoothstep(0.06, 0.16, xd)) * step(max(abs(xq.x), abs(xq.y)), 0.75) * blade;
      col = mix(col, vec3(1.0, 0.92, 0.9), cross);
      alpha = max(alpha, cross);
    }
    // Pending: an open blade with a light running out along the shaft to it.
    if (pending > 0.5 && waiting < 0.5) {
      float run = fract(uTime * 0.6) * (1.0 - uCalm);
      float dot1 = exp(-pow((along / max(vLen, 1e-3) - run) * 9.0, 2.0)) * shaft * 2.0;
      col += vec3(0.13, 0.83, 0.93) * dot1;
      alpha = max(alpha, dot1);
    }
  }
  // A result that just came back: the blade flashes, and a light runs back along the shaft to the hull.
  if (vResult < 1.6 && uCalm < 0.5) {
    float f = exp(-vResult * 2.2);
    float back = clamp(1.0 - vResult / 0.8, 0.0, 1.0);
    float runBack = exp(-pow((along / max(vLen, 1e-3) - back) * 8.0, 2.0)) * shaft * step(vResult, 0.8);
    col += vec3(1.0, 0.95, 0.8) * (blade * f + runBack * 1.6);
    alpha = max(alpha, max(blade * f, runBack));
  }
  // Highlighted (flown to, or the inspector's): a bright outline.
  if (bit(vFlags, 128.0) > 0.5) {
    col += vec3(1.0, 0.96, 0.88) * rim * 1.2;
    alpha = max(alpha, rim);
  }
  // Sandboxed (L1): the shaft wears verdigris.
  if (bit(vFlags, 256.0) > 0.5) col = mix(col, vec3(0.42, 1.0, 0.88) * 0.8, shaft * 0.7);
  col *= vFade;
  alpha *= vFade;
  if (alpha < 0.01) discard;
  gl_FragColor = vec4(col, clamp(alpha, 0.0, 1.0));
}
`

// ---------------------------------------------------------------- marks on a bench (theseus-hnof)
//
// A small sign at a bench's starboard rail that stays: a rose pennant where its turn had a failure (a failed tool call,
// or the turn failed), an amber lamp where a call of it waits for the operator, and a violet spark, for a few seconds,
// where its turn recalled memory. aMark: x kind (0 pennant, 1 lamp, 2 recall), y arrival (s), z dimmed.

export const MARK_VERT = /* glsl */ `
${VESSEL_COMMON}
uniform float uScale;
uniform float uPixel;
attribute float aIdx;
attribute vec3 aMark;
varying float vKind;
varying float vAge;
varying float vFade;
void main() {
  vec4 a = vRow(aIdx, 0.0);
  vec4 b = vRow(aIdx, 1.0);
  vKind = aMark.x;
  vAge = aMark.y > 0.0 ? uTime - aMark.y : 1e6;
  vec4 mv = viewMatrix * vec4(toWorld(a, position), 1.0);
  gl_Position = projectionMatrix * mv;
  float s = clamp(b.x * 0.42, 0.7, 1.6);
  if (vKind > 1.5) s *= 1.0 + 1.5 * exp(-vAge * 1.5);
  gl_PointSize = clamp(s * uScale / -mv.z, 6.0 * uPixel, 40.0 * uPixel);
  float hullPx = a.w * uScale / max(1.0, -(viewMatrix * vec4(a.x, 0.0, a.y, 1.0)).z);
  vFade = smoothstep(60.0 * uPixel, 200.0 * uPixel, hullPx) * (1.0 - max(bit(b.z, 16.0), aMark.z) * 0.75);
  if (vKind > 1.5) vFade *= clamp(1.0 - (vAge - 4.0) / 2.0, 0.0, 1.0);
}
`

export const MARK_FRAG = /* glsl */ `
uniform float uTime;
uniform float uCalm;
varying float vKind;
varying float vAge;
varying float vFade;
void main() {
  vec2 uv = gl_PointCoord * 2.0 - 1.0;
  uv.y = -uv.y;
  vec3 col = vec3(0.0);
  float a = 0.0;
  if (vKind < 0.5) {
    // A pennant: a pole and a swallow-tailed flag, rose.
    float pole = (1.0 - smoothstep(0.05, 0.11, abs(uv.x + 0.55))) * step(-0.9, uv.y) * step(uv.y, 0.85);
    vec2 f = vec2(uv.x + 0.5, uv.y - 0.38);
    float wave = 0.06 * sin(f.x * 6.0 + uTime * 3.0 * (1.0 - uCalm));
    float inFlag = step(0.0, f.x) * step(f.x, 1.25) * step(abs(f.y + wave) , 0.42 * (1.0 - f.x / 1.6));
    float notch = step(1.0 - abs(f.y + wave) * 1.4, f.x * 0.82);
    float flag = inFlag * (1.0 - notch * 0.0);
    col = vec3(0.95, 0.85, 0.7) * pole * 0.8 + vec3(1.0, 0.36, 0.44) * flag;
    a = max(pole * 0.85, flag);
  } else if (vKind < 1.5) {
    // A lamp: amber, swinging gently while it waits.
    float r = length(uv - vec2(0.0, 0.05 * sin(uTime * 2.0) * (1.0 - uCalm)));
    float core = 1.0 - smoothstep(0.22, 0.42, r);
    float halo = exp(-r * r * 3.0) * (0.55 + 0.25 * sin(uTime * 2.4) * (1.0 - uCalm));
    col = vec3(1.0, 0.77, 0.15) * (core + halo);
    a = core + halo * 0.7;
  } else {
    // Memory recalled: a violet spark that rings out and fades.
    float r = length(uv);
    float ring = 1.0 - smoothstep(0.0, 0.1, abs(r - clamp(vAge / 1.4, 0.2, 0.95)));
    float core = (1.0 - smoothstep(0.1, 0.35, r)) * exp(-vAge * 0.4);
    col = vec3(0.75, 0.6, 1.0) * (ring + core);
    a = max(ring * 0.9, core);
  }
  col *= vFade;
  a *= vFade;
  if (a < 0.01) discard;
  gl_FragColor = vec4(col, clamp(a, 0.0, 1.0));
}
`
