const VeinStudy = (() => {
  const defaults = Object.freeze({ blue: 20, red: 10, width: 100, reach: 65 });

  const shader = `
uniform vec4 uVeins;

// Each path has a compact extent, a tapered core and a lower-contrast halo.
vec2 veinPath(float axis, vec2 extent, vec3 controls) {
  float t = clamp((axis - extent.x) / (extent.y - extent.x), 0.0, 1.0);
  float envelope = smoothstep(0.0, .16, t) * (1.0 - smoothstep(.77, 1.0, t));
  float center = mix(controls.x, controls.y, t) + 4.0 * t * (1.0 - t) * controls.z;
  center += .10 * controls.z * sin(18.85 * t);
  return vec2(center, envelope);
}

float veinProfile(float distance, float width, float envelope, float pixel) {
  width *= uVeins.z * mix(.36, 1.0, envelope);
  float edge = max(.22 * width, .45 * pixel);
  float core = 1.0 - smoothstep(max(width - edge, 0.0), width + edge, distance);
  float haze = 1.0 - smoothstep(width, 2.4 * width + edge, distance);
  float coverage = min(1.0, width / max(.55 * pixel, .00001));
  return envelope * coverage * (.84 * core + .16 * haze);
}

float veinAround(float axis, vec2 polar, vec2 extent, vec3 controls,
                 float width, float pixel) {
  vec2 path = veinPath(axis, extent, controls);
  // Shortest arc distance is continuous at the cylindrical seam.
  float angle = abs(fract((polar.x - path.x) / 6.2831853 + .5) - .5) * 6.2831853;
  return veinProfile(angle * polar.y, width, path.y, pixel);
}

float veinFlat(vec2 point, vec2 extent, vec3 controls, float width, float pixel) {
  vec2 path = veinPath(point.y, extent, controls);
  return veinProfile(abs(point.x - path.x), width, path.y, pixel);
}

vec2 veinCoordinate(vec2 section) {
  float radius = length(section);
  // Keep near-axis points and atan(0, 0) out of the cylindrical calculation.
  return vec2(atan(section.y, section.x + .000001), max(.014, radius));
}

vec3 veinAlbedo(vec3 skin, vec3 rest) {
  if (uVeins.x + uVeins.y <= 0.0) return skin;
  // Derivatives are evaluated before position-dependent branches.
  float pixel = max(length(dFdx(rest)), length(dFdy(rest)));
  if (rest.y >= 1.23) return skin;

  float y = rest.y, x = abs(rest.x);
  float side = step(0.0, rest.x);
  float reach = smoothstep(.05, 1.0, uVeins.w);
  vec2 pigment = vec2(0.0);

  // These axes follow the wrists and forearms in the preview's standing mesh.
  if (x > .14 && y > .62 && y < 1.03) {
    float axisX = mix(.242, .169, clamp((y - .645) / .275, 0.0, 1.0));
    float axisZ = mix(-.058, .013, smoothstep(.685, .89, y))
                + .022 * (1.0 - smoothstep(.64, .696, y));
    vec2 polar = veinCoordinate(vec2(x - axisX, rest.z - axisZ));
    polar.x += mix(.0, .43 * smoothstep(.65, .88, y), side);
    float fade = smoothstep(.145, .175, x) * (1.0 - smoothstep(.91, 1.0, y));

    float trunk = veinAround(y, polar, vec2(.651, .908), vec3(-1.27, -2.01, .29), .00155, pixel);
    float fork = veinAround(y, polar, vec2(.703, .838), vec3(-1.22, -.47, .06), .00091, pixel);
    float shortFork = veinAround(y, polar, vec2(.664, .744), vec3(-1.26, -2.34, -.14), .00083, pixel);
    float dorsal = veinAround(y, polar, vec2(.658, .818), vec3(.92, 1.79, -.24), .00128, pixel);
    float branches = max(.84 * fork, max(.76 * shortFork, .80 * dorsal));
    pigment.x = max(trunk, reach * branches) * fade;

    float warm = veinAround(y, polar, vec2(.660, .827), vec3(-.92, -.06, .18), .00110, pixel);
    float warmFork = veinAround(y, polar, vec2(.714, .792), vec3(-.45, -1.24, -.06), .00077, pixel);
    pigment.y = max(warm, .80 * reach * warmFork) * fade * mix(.68, .91, side);
  }

  if (y < .75 && x < .15) {
    float axisZ = mix(.035, .007, smoothstep(.24, .54, y));
    vec2 polar = veinCoordinate(vec2(x - .068, rest.z - axisZ));
    polar.x += mix(.0, -.47 * smoothstep(.06, .42, y), side);
    float fade = smoothstep(.017, .037, x) * (1.0 - smoothstep(.11, .14, x));

    float ankle = veinAround(y, polar, vec2(.049, .29), vec3(-1.79, -.86, -.23), .00139, pixel);
    float fork = veinAround(y, polar, vec2(.107, .227), vec3(-1.75, -2.55, -.22), .00091, pixel);
    float calf = veinAround(y, polar, vec2(.18, .46), vec3(.78, 1.64, .27), .00115, pixel);
    float thigh = veinAround(y, polar, vec2(.47, .72), vec3(-1.64, -1.10, -.21), .00115, pixel);
    pigment.x = max(pigment.x, fade * max(max(.84 * ankle, .56 * fork), reach * max(.60 * calf, .32 * thigh)));

    float warm = veinAround(y, polar, vec2(.047, .232), vec3(-.94, -1.54, .24), .00102, pixel);
    float warmFork = veinAround(y, polar, vec2(.166, .304), vec3(-1.20, -.39, .13), .00075, pixel);
    pigment.y = max(pigment.y, fade * max(.73 * warm, .52 * reach * warmFork));
  }

  if (reach > 0.0 && x < .15 && y > .78) {
    // Light traces on the side of the torso and at the collar on the front.
    vec2 point = vec2(x + mix(0.0, .008, side), y);
    float anterior = 1.0 - smoothstep(-.053, -.024, rest.z);
    float sidewall = smoothstep(.085, .125, x);
    float flank = veinFlat(point, vec2(.80, 1.026), vec3(.109, .126, .012), .00146, pixel);
    float collar = veinFlat(point, vec2(1.11, 1.217), vec3(.084, .049, .024), .00124, pixel);
    float collarFork = veinFlat(point, vec2(1.148, 1.194), vec3(.089, .115, .005), .00088, pixel);
    pigment.x = max(pigment.x, .37 * reach * max(sidewall * flank, anterior * max(collar, .68 * collarFork)));
    float warm = veinFlat(point, vec2(.83, 1.033), vec3(.129, .119, -.008), .00111, pixel);
    pigment.y = max(pigment.y, .30 * reach * max(sidewall, anterior) * warm);
  }

  float neckFade = 1.0 - smoothstep(1.17, 1.23, y);
  vec2 weight = pigment * uVeins.xy * vec2(.38, .36) * neckFade;
  vec3 blue = vec3(.060, .086, .129);
  vec3 red = vec3(.138, .049, .063);
  return skin + (blue - skin) * weight.x + (red - skin) * weight.y;
}`;

  return { defaults, shader };
})();
