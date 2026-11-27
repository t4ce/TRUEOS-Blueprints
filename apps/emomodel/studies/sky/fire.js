const FireStudy = (() => {
  "use strict";

  const duration = 12;
  const maskSize = 64;
  const backdrop = [0.11, 0.10, 0.12];
  const exposure = 0.43;

  function createMask(gl) {
    // Bake periodic value noise into one small single-channel texture.
    const pixels = new Uint8Array(maskSize * maskSize);
    let seed = 0xd3a93278;
    function random() {
      seed ^= seed << 13;
      seed ^= seed >>> 17;
      seed ^= seed << 5;
      return (seed >>> 0) / 4294967296;
    }
    const layers = [4, 8, 16].map(size => ({
      size,
      cells: Float32Array.from({ length: size * size }, random),
    }));
    const smooth = t => t * t * (3 - 2 * t);
    const mix = (a, b, t) => a + (b - a) * t;
    function value(layer, x, y) {
      const { size, cells } = layer;
      const u = x * size / maskSize;
      const v = y * size / maskSize;
      const i = Math.floor(u), j = Math.floor(v);
      const at = (i, j) => cells[(j % size) * size + i % size];
      return mix(
        mix(at(i, j), at(i + 1, j), smooth(u - i)),
        mix(at(i, j + 1), at(i + 1, j + 1), smooth(u - i)),
        smooth(v - j),
      );
    }
    for (let y = 0; y < maskSize; y++) {
      for (let x = 0; x < maskSize; x++) {
        const noise = 0.60 * value(layers[0], x, y)
                    + 0.28 * value(layers[1], x, y)
                    + 0.12 * value(layers[2], x, y);
        pixels[y * maskSize + x] = Math.round(255 * noise);
      }
    }
    const mask = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, mask);
    gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, maskSize, maskSize, 0, gl.RED, gl.UNSIGNED_BYTE, pixels);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.REPEAT);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.REPEAT);
    return mask;
  }

  function linearColor(hex) {
    return [1, 3, 5].map(start => {
      const value = parseInt(hex.slice(start, start + 2), 16) / 255;
      return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
    });
  }

  function frame(seconds, pattern, azimuth) {
    const phase = seconds / duration;
    const wave = phase * 2 * Math.PI;
    const pulse = 0.88 + 0.18 * Math.sin(7 * wave + 0.6)
                       + 0.10 * Math.sin(19 * wave + 1.8)
                       + 0.06 * Math.sin(31 * wave);
    const sway = 0.11 * Math.sin(3 * wave) + 0.05 * Math.sin(5 * wave + 1.0);
    const angle = azimuth + sway;
    return {
      ...pattern,
      strength: pattern.strength * pulse,
      phase,
      position: [-1.08 * Math.sin(angle), 0.42 + 0.07 * Math.sin(4 * wave + 0.5), -1.08 * Math.cos(angle)],
      across: [-Math.cos(azimuth), 0, Math.sin(azimuth)],
    };
  }

  const shader = `
uniform sampler2D uFireMask;
uniform vec3 uFirePosition, uFireAcross, uFireEmber, uFireFlare;
uniform float uFireStrength, uFireContrast, uFireScale, uFirePhase;

vec3 fireLighting(vec3 world, vec3 n, vec3 skin) {
  vec3 delta = uFirePosition - world;
  float distanceSquared = dot(delta, delta);
  vec3 l = delta * inversesqrt(max(distanceSquared, .0001));
  float facing = dot(n, l);
  float wrap = clamp((facing + .22) / 1.22, 0.0, 1.0);
  float directed = mix(wrap, max(facing, 0.0), .72);
  float falloff = 1.0 / (.58 + .56 * distanceSquared);

  // The same repeating alpha map drifts upward at two scales in world space.
  vec2 p = vec2(dot(world, uFireAcross), world.y) * uFireScale;
  vec2 broadUV = p * vec2(.86, .45) - vec2(0.0, 2.0 * uFirePhase);
  vec2 detailUV = p * vec2(1.71, .69) + vec2(.39, .48 - 3.0 * uFirePhase);
  float density = .71 * texture(uFireMask, broadUV).r
                + .29 * texture(uFireMask, detailUV).r;
  float alpha = clamp(.5 + (density - .5) * (1.0 + 3.0 * uFireContrast), 0.0, 1.0);
  vec3 tint = mix(uFireEmber, uFireFlare, smoothstep(.25, .86, alpha));
  float energy = (0.12 + .88 * alpha) * uFireStrength * falloff;

  // A broad skin highlight follows the hidden light as it moves.
  vec3 halfVector = normalize(l + uBack);
  float h = max(0.0, dot(n, halfVector));
  float h2 = h * h, h4 = h2 * h2;
  return tint * energy * (skin * directed + .10 * h4 * h4 * directed);
}
`;

  return { backdrop, exposure, duration, createMask, linearColor, frame, shader };
})();
