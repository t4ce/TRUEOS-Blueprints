import { loadWingOutline, wingVertexSource } from "./motion.js";

const fragmentHeader = `#version 300 es
precision highp float;
out vec4 color;
`;

const ribbonFragment = fragmentHeader + `
in vec4 v_ribbon;
in vec2 v_tear_uv;
flat in float v_sky_shift;
uniform sampler2D u_f2_density;
uniform sampler2D u_sunrise;
uniform sampler2D u_twilight;
uniform sampler2D u_universe;
uniform sampler2D u_tear_mask;
uniform int u_sky;
uniform float u_repeat;
uniform float u_coverage;
uniform float u_opacity;
uniform float u_falloff;
uniform float u_tear_amount;

void main() {
    float age = clamp(v_ribbon.z, 0.0, 1.0);
    vec2 pan = vec2(0.0, v_sky_shift);
    vec2 sky_repeat_uv = fract(v_ribbon.xy * u_repeat + pan);
    vec3 rgb;
    float density;
    float sky_opacity = 1.0;
    if (u_sky == 2) {
        density = texture(u_f2_density, sky_repeat_uv).r;
        float nebula = smoothstep(0.035, 0.30, density);
        float sparks = smoothstep(0.30, 0.68, density);
        vec3 ink = vec3(0.029, 0.018, 0.071);
        vec3 red = vec3(0.85, 0.055, 0.08);
        vec3 ember = vec3(1.0, 0.61, 0.25);
        rgb = mix(mix(ink, red, nebula), ember, sparks);
        sky_opacity = mix(0.56, 1.0, nebula);
    } else {
        if (u_sky == 0) {
            vec2 sky_uv = vec2(0.18 + v_ribbon.x, 0.78 - 0.69 * age + 0.35 * v_ribbon.y);
            rgb = texture(u_sunrise, fract((sky_uv - 0.5) * u_repeat + 0.5 - pan)).rgb;
        } else if (u_sky == 1) {
            rgb = texture(u_twilight, sky_repeat_uv).rgb;
        } else {
            rgb = texture(u_universe, sky_repeat_uv).rgb;
        }
        density = dot(rgb, vec3(0.2126, 0.7152, 0.0722));
    }
    float threshold = 0.8 * (1.0 - u_coverage);
    float coverage = smoothstep(threshold, threshold + 0.14, density + 0.14)
                   * smoothstep(0.0, 0.05, u_coverage);
    float fade = smoothstep(0.0, 0.055, age) * pow(1.0 - age, u_falloff);
    float tear = 1.0;
    if (u_tear_amount > 0.0) {
        float mask = texture(u_tear_mask, v_tear_uv).r;
        float erosion = smoothstep(0.06, 0.57, age) * u_tear_amount;
        float cut = mix(-0.11, 0.74, erosion);
        float feather = clamp(fwidth(mask) * 0.75, 0.025, 0.16);
        tear = smoothstep(cut - feather, cut + feather, mask);
    }
    float alpha = u_opacity * v_ribbon.w * fade * sky_opacity * coverage * tear;
    color = vec4(rgb * alpha, alpha);
}
`;

const outlineFragment = fragmentHeader + `
void main() {
    color = vec4(0.90, 0.91, 0.94, 1.0);
}
`;

const byId = (id) => document.getElementById(id);
const canvas = document.querySelector("canvas");
const phaseInput = byId("phase");
const phaseOutput = byId("phase-value");
const tearAngleOutput = byId("tear-angle");
const playButton = byId("play");
const status = byId("status");

const defaults = {
  lifetime: 590,
  opacity: 1,
  falloff: 1,
  stretch: 0.8,
  downstrokeOnly: false,
  sky: "sunrise",
  repeat: 1.35,
  skyPan: 1,
  coverage: 1,
  tearAmount: 0.74,
  tearRepeat: 4.05,
  tearDrift: 1.45,
  flapRate: 0.45,
  tilt: -58,
  backdrop: "slate",
};

const baseline = {
  lifetime: 140,
  opacity: 0.36,
  falloff: 2,
  stretch: 1,
  downstrokeOnly: true,
  sky: "twilight-f2",
  repeat: 1,
  skyPan: 0,
  coverage: 1,
  tearAmount: 0,
  tearRepeat: 1,
  tearDrift: 1,
  flapRate: 0.25,
  tilt: 35,
  backdrop: "slate",
};

const backgrounds = {
  slate: [0.325, 0.396, 0.475],
  green: [0.02, 0.67, 0.31],
  blue: [0.045, 0.085, 0.25],
};

const skyModes = { sunrise: 0, twilight: 1, "twilight-f2": 2, universe: 3 };

const percent = (value) => `${Math.round(100 * value)}%`;
const multiplier = (value) => `${value.toFixed(2)}×`;
const degrees = (value) => `${value}°`;
const formats = {
  lifetime: (value) => `${value} ms`,
  opacity: percent,
  falloff: (value) => value.toFixed(1),
  stretch: percent,
  repeat: multiplier,
  skyPan: percent,
  coverage: percent,
  tearAmount: percent,
  tearRepeat: multiplier,
  tearDrift: multiplier,
  flapRate: (value) => `${value.toFixed(2)} Hz`,
  tilt: degrees,
};

const settings = { ...defaults };
const startPhase = Number(phaseInput.defaultValue);
let playing = true;
let phase = startPhase;

function tearAngleForTilt(tilt) {
  const progress = Math.min(1, Math.max(0, (tilt + 65) / 130));
  const angle = 175 + 25 * progress;
  return angle > 180 ? angle - 360 : angle;
}

function showTearAngle() {
  tearAngleOutput.value = degrees(Number(tearAngleForTilt(settings.tilt).toFixed(1)));
}

function setSettings(values) {
  Object.assign(settings, values);
  for (const [key, value] of Object.entries(settings)) {
    const input = byId(key);
    if (input.type === "checkbox") {
      input.checked = value;
    } else {
      input.value = value;
    }
    const output = document.querySelector(`output[for="${key}"]`);
    if (output) output.value = formats[key](value);
  }
  showTearAngle();
}

function watchControls(draw) {
  for (const key of Object.keys(settings)) {
    const input = byId(key);
    input.addEventListener("input", () => {
      if (input.type === "checkbox") {
        settings[key] = input.checked;
      } else if (input.type === "range") {
        settings[key] = input.valueAsNumber;
        document.querySelector(`output[for="${key}"]`).value = formats[key](settings[key]);
      } else {
        settings[key] = input.value;
      }
      if (key === "tilt") showTearAngle();
      draw();
    });
  }
  byId("baseline").addEventListener("click", () => {
    setSettings(baseline);
    draw();
  });
  byId("reset").addEventListener("click", () => {
    setSettings(defaults);
    phase = startPhase;
    draw();
  });
  byId("copy").addEventListener("click", async () => {
    const json = JSON.stringify({
      ...settings,
      tearAngle: Number(tearAngleForTilt(settings.tilt).toFixed(1)),
      phase: Number(phase.toFixed(3)),
    }, null, 2);
    try {
      await navigator.clipboard.writeText(json);
      byId("copy").textContent = "Copied";
      setTimeout(() => { byId("copy").textContent = "Copy settings"; }, 1500);
    } catch {
      const dialog = byId("settings-dialog");
      const text = byId("settings-json");
      text.value = json;
      dialog.showModal();
      text.select();
    }
  });
  playButton.addEventListener("click", () => setPlaying(!playing));
  phaseInput.addEventListener("input", () => {
    setPlaying(false);
    phase = phaseInput.valueAsNumber;
    draw();
  });
}

function setPlaying(value) {
  playing = value;
  playButton.textContent = playing ? "Pause" : "Play";
}

function compile(gl, type, source) {
  const shader = gl.createShader(type);
  gl.shaderSource(shader, source);
  gl.compileShader(shader);
  if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
    const error = gl.getShaderInfoLog(shader);
    gl.deleteShader(shader);
    throw new Error(error);
  }
  return shader;
}

function link(gl, vertex, fragmentSource) {
  const fragment = compile(gl, gl.FRAGMENT_SHADER, fragmentSource);
  const program = gl.createProgram();
  gl.attachShader(program, vertex);
  gl.attachShader(program, fragment);
  gl.linkProgram(program);
  gl.deleteShader(fragment);
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    const error = gl.getProgramInfoLog(program);
    gl.deleteProgram(program);
    throw new Error(error);
  }
  const uniforms = {};
  for (let index = 0; index < gl.getProgramParameter(program, gl.ACTIVE_UNIFORMS); index++) {
    const { name } = gl.getActiveUniform(program, index);
    uniforms[name] = gl.getUniformLocation(program, name);
  }
  return { program, uniforms };
}

function createMesh(gl, params, indices) {
  const vao = gl.createVertexArray();
  gl.bindVertexArray(vao);
  gl.bindBuffer(gl.ARRAY_BUFFER, gl.createBuffer());
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(params), gl.STATIC_DRAW);
  gl.enableVertexAttribArray(0);
  gl.vertexAttribPointer(0, 3, gl.FLOAT, false, 0, 0);
  if (indices) {
    gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, gl.createBuffer());
    gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, new Uint16Array(indices), gl.STATIC_DRAW);
  }
  gl.bindVertexArray(null);
  return { vao, count: indices ? indices.length : params.length / 3 };
}

function createMeshes(gl) {
  const segments = 64;
  const stroke = [];
  const ribbon = [];
  const indices = [];
  for (let index = 0; index <= segments; index++) {
    const t = index / segments;
    stroke.push(t, 0, -1, t, 0, 1);
    ribbon.push(t, 0, 0, t, 0.5, 0, t, 1, 0);
    if (index === segments) break;
    for (let row = 0; row < 2; row++) {
      const a = 3 * index + row;
      indices.push(a, a + 3, a + 1, a + 1, a + 3, a + 4);
    }
  }
  const hinge = [0, 0, -1, 0, 0, 1, 1, 0, -1, 1, 0, 1];
  return {
    stroke: createMesh(gl, stroke),
    hinge: createMesh(gl, hinge),
    ribbon: createMesh(gl, ribbon, indices),
  };
}

function bindTexture(gl, unit, wrap) {
  gl.activeTexture(gl.TEXTURE0 + unit);
  gl.bindTexture(gl.TEXTURE_2D, gl.createTexture());
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, wrap);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, wrap);
}

async function fetchTexture(path) {
  const response = await fetch(new URL(path, import.meta.url));
  if (!response.ok) throw new Error(`${path.split("/").at(-1)} could not load (${response.status})`);
  return response;
}

async function loadMask(gl, path, unit, wrap) {
  const size = 256;
  const response = await fetchTexture(path);
  const data = new Uint8Array(await response.arrayBuffer());
  if (data.length !== size * size) throw new Error(`Unexpected mask dimensions for ${path}`);
  bindTexture(gl, unit, wrap);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, size, size, 0, gl.RED, gl.UNSIGNED_BYTE, data);
}

async function loadSky(gl, path, unit) {
  const response = await fetchTexture(path);
  const sky = await createImageBitmap(await response.blob());
  bindTexture(gl, unit, gl.CLAMP_TO_EDGE);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGB8, gl.RGB, gl.UNSIGNED_BYTE, sky);
  sky.close();
}

async function start() {
  document.documentElement.dataset.status = "loading";
  setSettings(defaults);
  const gl = canvas.getContext("webgl2", { alpha: false, antialias: true });
  if (!gl) throw new Error("This preview needs WebGL2.");
  const [curves] = await Promise.all([
    loadWingOutline(),
    loadMask(gl, "../../shaders/trueos/bfly-twilight.r8", 0, gl.CLAMP_TO_EDGE),
    loadSky(gl, "../dwn/600px-Sunrise_background.jpg", 1),
    loadMask(gl, "./wake-tear.r8", 2, gl.MIRRORED_REPEAT),
    loadSky(gl, "../dwn/600px-Twilight_background.png", 3),
    loadSky(gl, "../uni.jpg", 4),
  ]);
  const vertex = compile(gl, gl.VERTEX_SHADER, wingVertexSource);
  const stroke = link(gl, vertex, outlineFragment);
  const ribbon = link(gl, vertex, ribbonFragment);
  gl.deleteShader(vertex);
  const meshes = createMeshes(gl);
  gl.enable(gl.BLEND);
  gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
  let pixelRatio = 1;

  function resize() {
    const bounds = canvas.getBoundingClientRect();
    pixelRatio = Math.min(window.devicePixelRatio, 2);
    canvas.width = Math.max(1, Math.round(bounds.width * pixelRatio));
    canvas.height = Math.max(1, Math.round(bounds.height * pixelRatio));
    gl.viewport(0, 0, canvas.width, canvas.height);
    draw();
  }

  function drawPass({ program, uniforms: u }, drawStroke) {
    gl.useProgram(program);
    gl.uniform1i(u.u_stroke ?? null, Number(drawStroke));
    gl.uniform1f(u.u_time ?? null, phase / settings.flapRate);
    gl.uniform1f(u.u_flap_rate ?? null, settings.flapRate);
    gl.uniform1f(u.u_flap_angle ?? null, 32 * Math.PI / 180);
    gl.uniform1f(u.u_pitch ?? null, settings.tilt * Math.PI / 180);
    gl.uniform1f(u.u_roll ?? null, -10 * Math.PI / 180);
    gl.uniform1f(u.u_scale ?? null, 1.72);
    gl.uniform2f(u.u_viewport ?? null, canvas.width, canvas.height);
    gl.uniform1f(u.u_line_width ?? null, 1.35 * pixelRatio);
    if (!drawStroke) {
      gl.uniform1f(u.u_horizon, settings.lifetime / 1000);
      gl.uniform1f(u.u_motion_stretch, settings.stretch);
      gl.uniform1i(u.u_downstroke_only, Number(settings.downstrokeOnly));
      gl.uniform1i(u.u_f2_density, 0);
      gl.uniform1i(u.u_sunrise, 1);
      gl.uniform1i(u.u_tear_mask, 2);
      gl.uniform1i(u.u_twilight, 3);
      gl.uniform1i(u.u_universe, 4);
      gl.uniform1i(u.u_sky, skyModes[settings.sky]);
      gl.uniform1f(u.u_opacity, settings.opacity);
      gl.uniform1f(u.u_falloff, settings.falloff);
      gl.uniform1f(u.u_repeat, settings.repeat);
      gl.uniform1f(u.u_sky_pan, settings.skyPan);
      gl.uniform1f(u.u_coverage, settings.coverage);
      gl.uniform1f(u.u_tear_amount, settings.tearAmount);
      const angle = tearAngleForTilt(settings.tilt) * Math.PI / 180;
      const repeat = settings.tearRepeat;
      gl.uniform2f(u.u_tear_basis, repeat * Math.cos(angle), repeat * Math.sin(angle));
      gl.uniform1f(u.u_tear_drift, settings.tearDrift);
    }
    for (const [curveIndex, curve] of curves.entries()) {
      if (curve.hinge && !drawStroke) continue;
      const mesh = drawStroke ? (curve.hinge ? meshes.hinge : meshes.stroke) : meshes.ribbon;
      gl.bindVertexArray(mesh.vao);
      gl.uniform4fv(u.u_curve_x, curve.x);
      gl.uniform4fv(u.u_curve_y, curve.y);
      if (!drawStroke) gl.uniform1f(u.u_curve_index, curveIndex);
      for (const side of curve.hinge ? [0] : [-1, 1]) {
        gl.uniform1f(u.u_side, side);
        if (drawStroke) {
          gl.drawArrays(gl.TRIANGLE_STRIP, 0, mesh.count);
        } else {
          gl.drawElements(gl.TRIANGLES, mesh.count, gl.UNSIGNED_SHORT, 0);
        }
      }
    }
  }

  function draw() {
    gl.clearColor(...backgrounds[settings.backdrop], 1);
    gl.clear(gl.COLOR_BUFFER_BIT);
    if (settings.opacity > 0 && settings.coverage > 0 && settings.stretch > 0) {
      drawPass(ribbon, false);
    }
    drawPass(stroke, true);
    phaseInput.value = phase;
    phaseOutput.value = percent(phase);
  }

  watchControls(draw);
  new ResizeObserver(resize).observe(canvas);
  resize();
  playButton.disabled = false;
  phaseInput.disabled = false;
  status.hidden = true;
  document.documentElement.dataset.status = "ready";

  let lastFrame;
  function animate(now) {
    if (playing && lastFrame !== undefined) {
      const elapsed = Math.min((now - lastFrame) / 1000, 0.1);
      phase = (phase + elapsed * settings.flapRate) % 1;
      draw();
    }
    lastFrame = now;
    requestAnimationFrame(animate);
  }
  requestAnimationFrame(animate);
}

start().catch((error) => {
  document.documentElement.dataset.status = "error";
  status.textContent = error.message;
  console.error(error);
});
