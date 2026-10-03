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

export const SEA_FRAG = /* glsl */ `
uniform vec2 uTarget;
uniform float uDist;
uniform vec2 uCenter;
uniform float uRose;
uniform float uParts;
varying vec3 vWorld;
float part(float b) { return mod(floor(uParts / b + 0.001), 2.0); }
${HASH}
// A line every unit of a continuous coordinate, anti-aliased by that coordinate's own derivative (never the derivative
// of a fract(), which spikes where it wraps and draws false lines).
float lineAt(float coord, float fw, float w) {
  float d = abs(fract(coord - 0.5) - 0.5);
  return 1.0 - smoothstep(w * fw, (w + 1.2) * fw, d);
}
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
  col += cyan * (minor * 0.045 + major * 0.085) * near * part(1.0);
  // The logo's waves: rows of long sine lines, faint teal (one sine per row, no seams).
  float ws = 30.0 * exp2(max(0.0, floor(lod)));
  float wy = (p.y + sin(p.x / ws * 4.2) * ws * 0.11) / ws;
  float wave = lineAt(wy, fwidth(wy), 0.6);
  col += vec3(0.24, 0.49, 0.58) * wave * 0.16 * near * part(2.0);
  // An engraved compass rose under the fleet: thirty-two rays and two rings, in old gold.
  vec2 q = p - uCenter;
  float r = length(q);
  if (uRose > 0.0 && r < uRose * 1.08 && r > 1e-3 && part(4.0) > 0.5) {
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
  // A faint fixed sparkle on the water: stars reflected, from the logo's night. Only once a cell is a few pixels
  // across (smaller, a dot would spill past its cell and be cut into a dash).
  vec2 cell = floor(p / 9.0);
  float fwp = max(fwidth(p.x), fwidth(p.y));
  float cellPx = 9.0 / max(fwp, 1e-4);
  float h = hash12(cell);
  if (h > 0.93 && cellPx > 8.0 && part(8.0) > 0.5) {
    vec2 c = (cell + 0.2 + 0.6 * vec2(hash12(cell + 7.1), hash12(cell + 3.3))) * 9.0;
    float sd = length(p - c);
    float rad = min(0.18 + fwp * 1.2, 1.6);
    col += vec3(0.91, 0.79, 0.50) * (1.0 - smoothstep(rad * 0.5, rad, sd)) * 0.5 * near * smoothstep(8.0, 18.0, cellPx);
  }
  gl_FragColor = vec4(col, 1.0);
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
  if (rig > 2.5) return vec3(0.98, 0.44, 0.52) * 1.9;   // flare: rose
  if (rig > 1.5) return vec3(0.98, 0.75, 0.14) * 1.25;  // lantern: amber
  if (rig > 0.5) return vec3(0.13, 0.83, 0.93) * 1.45;  // under sail: neon cyan
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
  ec += vec3(0.9, 0.8, 0.5) * born * 2.0;

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
    vec3 cc = mix(vec3(0.98, 0.75, 0.14), vec3(0.96, 0.45, 0.71), 0.45) * 1.8;
    col += cc * chain;
    alpha = max(alpha, chain);
  }

  float hot = vRig > 0.5 ? 1.0 : 0.45;
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
    col += vec3(0.13, 0.83, 0.93) * 1.2 * ring * dash;
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
  vec3 foot = toWorld(a, vec3(a.w * 0.04, 0.25, 0.0));
  // A sail shows once its hull is big enough on screen to carry one (far out, the cyan rail says it).
  float px = a.w * uScale / max(1.0, -(viewMatrix * vec4(foot, 1.0)).z);
  vFade = smoothstep(90.0, 170.0, px);
  vec3 right = vec3(viewMatrix[0][0], viewMatrix[1][0], viewMatrix[2][0]);
  vec3 up = vec3(viewMatrix[0][1], viewMatrix[1][1], viewMatrix[2][1]);
  float w = a.w * 0.3 + 0.4;
  float h = a.w * 0.36 + 0.5;
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
  vec3 col = ivory * 0.13 * inSail + cyan * 1.25 * edge + ivory * 0.75 * (mast + yard);
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
  float dim = bit(b.z, 16.0);
  gl_PointSize = clamp(s * uScale / -mv.z, 2.2 * uPixel, 72.0 * uPixel);
  vColor = aColor * (1.0 - dim * 0.75);
  vFlags = aFlags;
  vAge = vec2(age, aTimes.y > 0.0 ? uTime - aTimes.y : -1.0);
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
float bit(float flags, float b) { return mod(floor(flags / b + 0.001), 2.0); }
float hexDist(vec2 p) { p = abs(p); return max(p.x * 0.8660254 + p.y * 0.5, p.y); }
void main() {
  vec2 uv = gl_PointCoord * 2.0 - 1.0;
  float r = length(uv);
  float k = mix(1.0, 2.3, vRing);           // the core shrinks inside a ring
  float rc = r * k;
  float core = 1.0 - smoothstep(0.18, 0.42, rc);
  float halo = exp(-rc * rc * 5.0) * 0.7;
  vec3 col = vColor * (core * 1.25 + halo);
  float alpha = core + halo * 0.8;
  // L1: a hexagonal shield, verdigris neon. A cancel verified collapses it.
  if (bit(vFlags, 1.0) > 0.5) {
    float shrink = vAge.y >= 0.0 ? clamp(1.0 - vAge.y / 1.2, 0.0, 1.0) : 1.0;
    float hd = hexDist(uv) - 0.78 * shrink;
    float ring = 1.0 - smoothstep(0.03, 0.09, abs(hd));
    float fill = (1.0 - smoothstep(0.0, 0.04, hd)) * 0.10;
    vec3 sc = vec3(0.37, 0.92, 0.80) * 1.5;
    col += sc * (ring + fill) * shrink;
    alpha = max(alpha, (ring + fill) * shrink);
  }
  // External text: a warning ring in magenta.
  if (bit(vFlags, 2.0) > 0.5) {
    float ring = 1.0 - smoothstep(0.04, 0.11, abs(r - 0.66));
    vec3 mc = vec3(0.96, 0.45, 0.71) * 2.0;
    col += mc * ring + mc * 0.25 * exp(-r * r * 2.5);
    alpha = max(alpha, ring);
  }
  // A job running now: a turning gear.
  if (bit(vFlags, 4.0) > 0.5) {
    float ang = atan(uv.y, uv.x) + uTime * 1.6 * (1.0 - uCalm);
    float teeth = step(0.5, fract(ang / 6.2831853 * 10.0));
    float rr = 0.80 + teeth * 0.1;
    float gear = 1.0 - smoothstep(0.03, 0.08, abs(r - rr + 0.05));
    vec3 gc = vec3(0.84, 0.65, 0.28) * 1.8;
    col += gc * gear;
    alpha = max(alpha, gear);
  }
  // Highlighted (flown to, or the inspector's): a bright cross-hair ring.
  if (bit(vFlags, 16.0) > 0.5) {
    float ring = 1.0 - smoothstep(0.02, 0.06, abs(r - 0.92));
    col += vec3(1.0, 0.95, 0.85) * 1.6 * ring;
    alpha = max(alpha, ring);
  }
  // A fact arrived: a flare, then a ring that runs out and fades.
  if (vAge.x < 2.0) {
    float ring = 1.0 - smoothstep(0.0, 0.08, abs(r - mix(0.2, 0.98, clamp(vAge.x / 1.1, 0.0, 1.0))));
    float f = exp(-vAge.x * 2.0);
    col += vec3(1.0, 0.92, 0.7) * (ring * f * 1.6 + core * f * 1.5);
    alpha = max(alpha, ring * f);
  }
  if (alpha < 0.01) discard;
  gl_FragColor = vec4(col, clamp(alpha, 0.0, 1.0));
}
`

// ---------------------------------------------------------------- lines (keels, oars, formation rings)

export const LINE_VERT = /* glsl */ `
${VESSEL_COMMON}
attribute float aIdx;
attribute vec4 aColor;
varying vec4 vColor;
void main() {
  vec3 w = position;
  float dim = 0.0;
  if (aIdx >= 0.0) {
    vec4 a = vRow(aIdx, 0.0);
    dim = bit(vRow(aIdx, 1.0).z, 16.0);
    w = toWorld(a, position);
  }
  vColor = vec4(aColor.rgb * (1.0 - dim * 0.7), aColor.a);
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
  if (burst < 3.0) {
    // The report lands: a run of gold from the task back to its parent.
    t = 1.0 - fract(aParam.x + burst * 0.9 * (1.0 - uCalm));
    col = vec3(0.89, 0.68, 0.31) * 2.2;
    a = 0.95 * exp(-burst * 0.5);
  } else if (aParam.y > 0.0) {
    t = fract(aParam.x + uTime * aParam.y * (1.0 - uCalm));
    a = 0.75;
  }
  float u = 1.0 - t;
  vec3 p = u * u * p0 + 2.0 * u * t * p1 + t * t * p2;
  vec4 mv = viewMatrix * vec4(p, 1.0);
  gl_Position = projectionMatrix * mv;
  gl_PointSize = clamp(aSize * uScale / -mv.z, 1.8 * uPixel, 9.0 * uPixel);
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
      // The lantern: it waits for the operator.
      l = vec3(-L * 0.5 - B * 0.35, B * 0.9 + 0.6, -B * 0.75);
      vColor = vec3(0.98, 0.75, 0.14) * 2.4;
      pulse = 0.78 + 0.22 * sin(uTime * 2.4) * (1.0 - uCalm);
      size = 1.6 + B * 0.45;
    } else if (b.y > 2.5) {
      // The flare: it failed.
      l = vec3(L * 0.1, B * 1.6 + 1.2, 0.0);
      vColor = vec3(0.98, 0.32, 0.38) * 2.4;
      size = 1.4 + B * 0.22;
    }
  } else if (aKind < 1.5) {
    if (c.y > 0.5) {
      l = aAt;
      vColor = vec3(0.65, 0.55, 0.98) * 2.2;
      pulse = 0.6 + 0.4 * sin(uTime * 7.0) * (1.0 - uCalm);
      size = 1.1;
    }
  } else {
    float age = c.x > 0.0 ? uTime - c.x : 1e6;
    if (age < 2.6) {
      l = vec3(L * 0.1, B * 1.0 + age * B * 1.6, 0.0);
      vColor = vec3(1.0, 0.4, 0.4) * 2.6;
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
  gl_FragColor = vec4(vColor * (core * 1.3 + halo * 0.7) * vA, a);
}
`
