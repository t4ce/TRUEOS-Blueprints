const FlareStudy = (() => {
  "use strict";

  const duration = 15;
  const cue = 1.35;
  const peak = cue + 0.21;
  const end = cue + 1.16;
  const backdrop = [0.105, 0.095, 0.12];

  function ease(from, to, time) {
    const t = Math.min(1, Math.max(0, (time - from) / (to - from)));
    return t * t * (3 - 2 * t);
  }

  function pulse(seconds) {
    if (!Number.isFinite(seconds)) return 0;
    const remainder = seconds % duration;
    const phase = remainder < 0 ? remainder + duration : remainder;
    if (phase <= cue || phase >= end) return 0;
    return ease(cue, peak, phase) * (1 - ease(peak + 0.07, end, phase));
  }

  const hairShader = `
uniform vec3 uCrown, uHalf;
uniform float uCrownRadius, uCrownGlint;

vec3 crownGlint(vec3 hairLinear, vec3 normal, vec3 rest, vec3 viewBack) {
  if (uCrownGlint <= 0.0) return hairLinear;
  vec3 delta = (rest - uCrown) / max(uCrownRadius, .0001);
  float along = dot(delta, vec3(.94, .27, -.21));
  float distance2 = 1.6 * dot(delta, delta) - .88 * along * along;
  float crownMask = 1.0 - smoothstep(.03, 1.0, distance2);
  if (crownMask <= 0.0) return hairLinear;

  vec3 n = normal * inversesqrt(max(dot(normal, normal), .0001));
  float alignment = clamp(abs(dot(n, uHalf)), 0.0, 1.0);
  // A tight view-dependent sheen over a softer reflection that survives stylized hair normals.
  float sheen = .42 + .38 * pow(alignment, 8.0) + .20 * pow(alignment, 26.0);
  float ribbon = 1.0 - smoothstep(.08, .55, abs(delta.y + .24 * delta.x));
  float facing = .78 + .22 * smoothstep(.06, .42, abs(dot(n, viewBack)));
  float strength = crownMask * sheen * facing * (.45 + .55 * ribbon) * uCrownGlint;
  return hairLinear + vec3(1.00, .74, .45) * strength;
}`;

  const opticsVertex = `#version 300 es
out vec2 vFlareUv;
void main() {
  vec2 corner = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
  vFlareUv = corner;
  gl_Position = vec4(corner * 2.0 - 1.0, 0.0, 1.0);
}`;

  const opticsFragment = `#version 300 es
precision highp float;
uniform vec2 uFlareSource, uFlareExtent;
uniform float uFlarePower, uFlareGhosts;
in vec2 vFlareUv;
out vec4 pixel;

float aperture(vec2 p, float radius, float contour) {
  // Soft six-sided aperture reflection in axes rotated away from the streak.
  vec2 q = abs(vec2(.8660254 * p.x + .5 * p.y, -.5 * p.x + .8660254 * p.y));
  float r = max(q.y, .8660254 * q.x + .5 * q.y);
  float fill = 1.0 - smoothstep(.30 * radius, radius, r);
  float rim = 1.0 - smoothstep(.05 * radius, .28 * radius, abs(r - .80 * radius));
  return mix(fill, rim, contour);
}

void main() {
  if (uFlarePower <= 0.0) discard;
  // Use the shorter viewport dimension so circles remain round and the design scales.
  vec2 aspect = uFlareExtent / max(1.0, min(uFlareExtent.x, uFlareExtent.y));
  vec2 p = (vFlareUv - uFlareSource) * aspect;
  vec2 towardCenter = (.5 - uFlareSource) * aspect;
  float r2 = dot(p, p);
  float core = 1.0 - smoothstep(.00001, .00030, r2);
  float glow = .32 * (1.0 - smoothstep(0.0, .0040, r2));
  float haze = .13 * (1.0 - smoothstep(0.0, .0256, r2));
  float streak = .20 * (1.0 - smoothstep(.001, .009, abs(p.y)))
                     * (1.0 - smoothstep(.009, .25, abs(p.x)));
  float spine = .14 * (1.0 - smoothstep(.001, .004, abs(p.x)))
                    * (1.0 - smoothstep(.007, .058, abs(p.y)));

  vec3 light = core * vec3(1.0, .985, .93)
             + (glow + haze) * vec3(1.0, .69, .36)
             + (streak + spine) * vec3(.98, .78, .51);
  float opacity = core + glow + haze + streak + spine;

  if (uFlareGhosts > 0.0) {
    vec2 q = p - towardCenter * .42;
    float rose = .17 * aperture(q, .038, .19);
    q = p - towardCenter * .73;
    float gold = .13 * aperture(q, .025, .30);
    q = p - towardCenter * 1.23;
    float jade = .12 * aperture(q, .067, .76);
    q = p - towardCenter * 1.62;
    float violet = .11 * aperture(q, .043, .48);
    float axisFade = smoothstep(.015, .09, length(towardCenter));
    float ghosts = uFlareGhosts * axisFade;
    light += ghosts * (rose * vec3(1.0, .57, .48) + gold * vec3(.98, .84, .43)
                       + jade * vec3(.43, .88, .72) + violet * vec3(.65, .61, .96));
    opacity += ghosts * (rose + gold + jade + violet);
  }

  if (opacity <= .0001) discard;
  float alpha = min(1.0, opacity * uFlarePower);
  pixel = vec4(light * (alpha / opacity), alpha);
}`;

  return { duration, cue, peak, end, backdrop, pulse, hairShader, opticsVertex, opticsFragment };
})();
