function readPoints(source, name, expected) {
  const definition = source.match(new RegExp(`const ${name}: [^=]+ = \\[([\\s\\S]*?)\\n\\];`));
  if (!definition) throw new Error(`Missing butterfly geometry ${name}`);
  const points = Array.from(definition[1].matchAll(/\[([-+\d.eE]+),\s*([-+\d.eE]+)\]/g),
    ([, x, y]) => [Number(x), Number(y)]);
  if (points.length !== expected) throw new Error(`Unexpected ${name} control count`);
  return points;
}

export async function loadWingOutline() {
  const response = await fetch(new URL("../../src/geometry.rs", import.meta.url));
  if (!response.ok) throw new Error(`Butterfly geometry could not load (${response.status})`);
  const source = await response.text();
  const path = readPoints(source, "FILL_WING", 10);
  const start = path[0];
  const end = path.at(-1);
  const root = source.match(/const ROOT_Y: f64 = ([-+\d.eE]+);/);
  if (!root) throw new Error("Missing butterfly hinge origin");
  const rootY = Number(root[1]);

  const curve = (points, hinge = false) => ({
    x: Float32Array.from(points, ([x]) => x - start[0]),
    y: Float32Array.from(points, ([, y]) => rootY - y),
    hinge,
  });
  return [curve(path.slice(0, 4)), curve(path.slice(3, 7)), curve(path.slice(6, 10)),
    curve([end, end, start, start], true)];
}

export const wingVertexSource = `#version 300 es
precision highp float;

layout(location = 0) in vec3 a_param;
uniform vec4 u_curve_x;
uniform vec4 u_curve_y;
uniform float u_side;
uniform float u_time;
uniform float u_horizon;
uniform float u_motion_stretch;
uniform float u_flap_rate;
uniform float u_flap_angle;
uniform float u_pitch;
uniform float u_roll;
uniform float u_scale;
uniform vec2 u_viewport;
uniform float u_line_width;
uniform float u_sky_pan;
uniform float u_tear_drift;
uniform vec2 u_tear_basis;
uniform float u_curve_index;
uniform int u_stroke;
uniform int u_downstroke_only;

out vec4 v_ribbon;
out vec2 v_tear_uv;
flat out float v_sky_shift;

const float TAU = 6.28318530718;
const float WING_TIP = 0.6622233225;

vec2 outline(float t) {
    if (u_side == 0.0) {
        return mix(vec2(u_curve_x.x, u_curve_y.x), vec2(u_curve_x.w, u_curve_y.w), t);
    }
    float s = 1.0 - t;
    vec4 basis = vec4(s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t);
    return vec2(dot(u_curve_x, basis), dot(u_curve_y, basis));
}

float phase(float time) {
    float offset = u_side < 0.0 ? 0.135 : 0.0;
    return TAU * fract(u_flap_rate * time + offset);
}

float flap_at(float time) {
    return u_flap_angle * sin(phase(time));
}

vec2 fold_at(float flap) {
    return vec2(cos(flap), sin(flap));
}

vec4 wing_clip(vec2 point, vec2 fold) {
    vec3 wing = vec3(u_side * point.x * fold.x, point.y, point.x * fold.y);
    float pc = cos(u_pitch);
    float ps = sin(u_pitch);
    float rc = cos(u_roll);
    float rs = sin(u_roll);
    vec3 view = vec3(wing.x, pc * wing.y - ps * wing.z, ps * wing.y + pc * wing.z);
    vec2 screen = vec2(rc * view.x - rs * view.y, rs * view.x + rc * view.y);
    screen *= u_scale * vec2(u_viewport.y / u_viewport.x, 1.0);
    return vec4(screen, 0.0, 1.0 - 0.22 * view.z);
}

vec2 pixel_position(vec4 clip) {
    return (clip.xy / clip.w) * (0.5 * u_viewport);
}

void main() {
    float t = a_param.x;
    float age = a_param.y;
    vec2 point = outline(t);
    v_ribbon = vec4(0.0);
    v_tear_uv = vec2(0.0);
    float head_flap = flap_at(u_time);
    // Pan displacement tracks flap angle so its rate follows signed wing speed.
    v_sky_shift = 0.18 * u_sky_pan * head_flap;
    vec2 head_fold = fold_at(head_flap);
    if (u_stroke != 0) {
        vec4 clip = wing_clip(point, head_fold);
        vec2 a = pixel_position(wing_clip(outline(max(0.0, t - 0.002)), head_fold));
        vec2 b = pixel_position(wing_clip(outline(min(1.0, t + 0.002)), head_fold));
        vec2 span = b - a;
        vec2 normal = vec2(-span.y, span.x) / max(length(span), 0.0001);
        clip.xy += normal * (a_param.z * u_line_width * clip.w) / u_viewport;
        gl_Position = clip;
        return;
    }

    vec2 tip = vec2(WING_TIP, 0.0);
    vec2 mid_fold = fold_at(mix(head_flap, flap_at(u_time - 0.5 * u_horizon), u_motion_stretch));
    vec2 tail_fold = fold_at(mix(head_flap, flap_at(u_time - u_horizon), u_motion_stretch));
    vec2 head = pixel_position(wing_clip(tip, head_fold));
    vec2 mid = pixel_position(wing_clip(tip, mid_fold));
    vec2 tail = pixel_position(wing_clip(tip, tail_fold));
    float sweep = length(head - mid) + length(mid - tail);
    if (u_downstroke_only != 0 && cos(phase(u_time)) >= 0.0) sweep = 0.0;
    float motion = smoothstep(1.5, 12.0, sweep);
    vec2 uv = vec2(0.87 * point.x + 0.67 * point.y, 0.44 * point.y + 0.29 * age);
    v_ribbon = vec4(uv, age, motion);
    // X follows the sweep. Y tracks the outline across the adjacent cubics.
    float drift = u_tear_drift * sin(phase(u_time) - 1.6 * age);
    float outline_position = (u_curve_index + t) / 3.0;
    vec2 tear_uv = vec2(0.17 + 0.66 * age + 0.14 * drift,
                        0.76 * outline_position + (u_side < 0.0 ? 0.10 : 0.21) + 0.04 * age);
    // Rotate and repeat the mask around its center in the wake's local coordinates.
    float c = u_tear_basis.x;
    float s = u_tear_basis.y;
    v_tear_uv = mat2(c, -s, s, c) * (tear_uv - 0.5) + 0.5;
    // The native tessellator sweeps two age intervals through the actual flap poses.
    vec2 fold = age < 0.5 ? mix(head_fold, mid_fold, 2.0 * age)
                         : mix(mid_fold, tail_fold, 2.0 * age - 1.0);
    gl_Position = wing_clip(point, fold);
}
`;
