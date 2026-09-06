// ============================================================
//  "Hex Array Pulse" — geometric motion graphics loop (v2)
//  Perfect 8-second loop: frame t=0 is bit-identical to t=8. Mouse orbits.
//
//  GEOMETRY: a circular array (radius ARRAYR cells) of alternating rounded
//  boxes and hex prisms standing on a dark glossy floor, plus a ring of eight
//  tori orbiting above the array centre.
//  MOTION (each its own frequency, every angle an INTEGER multiple of phase):
//    1. radial height wave travelling outward through the array  (2 x phase)
//       + a slower diagonal swell across it                      (1 x phase)
//    2. per-object spin, direction + start angle per cell        (1 x phase)
//    3. torus ring orbit (1x) with per-torus tumble (3x) and bob (2x)
//    4. camera drift (yaw / pitch / dolly) + palette hue drift  (1 x phase)
//  COLOUR: per-instance hue (radial ring index + checkerboard + drift),
//  height ramp (dark feet -> bright caps), crest caps glow, contrasting-hue
//  fresnel rim, glossy floor reflection.
// ============================================================

#define TAU     6.28318530718
#define LOOP    8.0
#define STEPS   100      // primary march
#define RSTEPS  40       // floor reflection march
#define MAXD    34.0
#define SURF    0.002
#define CELL    1.0
#define NRING   8.0
#define ARRAYR  5.6      // array radius in cells — bounded so it has a silhouette

float gPhase;      // TAU * iTime / LOOP  — every animated term is periodic in this
float gDrift;      // iTime / LOOP        — hue drift, one full palette cycle per loop

// ---------- helpers ----------
mat2 rot(float a){ float c = cos(a), s = sin(a); return mat2(c, -s, s, c); }

float hash12(vec2 p){
    vec3 p3 = fract(vec3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
}
float hash13(vec3 p){
    p = fract(p * 0.1031);
    p += dot(p, p.yzx + 33.33);
    return fract((p.x + p.y) * p.z);
}

// Explicit vivid stops. iq's cosine palette went grey/muddy on v1.
vec3 palette(float t){
    t = fract(t) * 4.0;
    vec3 c0 = vec3(1.00, 0.16, 0.56);   // magenta
    vec3 c1 = vec3(0.44, 0.20, 1.00);   // electric violet
    vec3 c2 = vec3(0.08, 0.92, 0.96);   // cyan
    vec3 c3 = vec3(1.00, 0.62, 0.18);   // amber
    vec3 a = t < 1.0 ? c0 : (t < 2.0 ? c1 : (t < 3.0 ? c2 : c3));
    vec3 b = t < 1.0 ? c1 : (t < 2.0 ? c2 : (t < 3.0 ? c3 : c0));
    return mix(a, b, smoothstep(0.0, 1.0, fract(t)));
}

// ---------- primitives ----------
float sdRoundBox(vec3 p, vec3 b, float r){
    vec3 q = abs(p) - b;
    return length(max(q, 0.0)) + min(max(q.x, max(q.y, q.z)), 0.0) - r;
}
// hex prism, axis along y, h.x = apothem, h.y = half height
float sdHexPrism(vec3 p, vec2 h){
    p = p.xzy;
    const vec3 k = vec3(-0.8660254, 0.5, 0.57735);
    p = abs(p);
    p.xy -= 2.0 * min(dot(k.xy, p.xy), 0.0) * k.xy;
    vec2 d = vec2(length(p.xy - vec2(clamp(p.x, -k.z * h.x, k.z * h.x), h.x)) * sign(p.y - h.x),
                  p.z - h.y);
    return min(max(d.x, d.y), 0.0) + length(max(d, 0.0));
}
float sdTorus(vec3 p, vec2 t){
    vec2 q = vec2(length(p.xz) - t.x, p.y);
    return length(q) - t.y;
}

// ---------- scene ----------
// per-cell half-height: two travelling waves on different frequencies
float cellHeight(vec2 id){
    float r  = length(id * CELL);
    float w1 = sin(2.0 * gPhase - r * 1.15);              // radial ripple, 2 crests per loop
    float w2 = sin(gPhase + (id.x - id.y) * 0.55);        // diagonal swell, 1 per loop
    return 0.42 + 0.24 * w1 + 0.10 * w2;                  // min 0.08: troughs become flat tiles
}

// one column of the array, in world space, for cell id
float column(vec3 p, vec2 id, out float hh, out float cb){
    hh = cellHeight(id);
    cb = mod(id.x + id.y, 2.0);                           // checkerboard: 0 box, 1 hex
    if (length(id + 0.5) > ARRAYR) return 1e5;            // circular array bound
    float hs = hash12(id);
    float spin = (hs < 0.5 ? -gPhase : gPhase) + hs * TAU;   // 1 turn per loop -> periodic
    vec3 q = vec3(p.x - (id.x + 0.5) * CELL, p.y - hh, p.z - (id.y + 0.5) * CELL);
    q.xz *= rot(spin);
    return cb < 0.5 ? sdRoundBox(q, vec3(0.26, hh - 0.05, 0.26), 0.05)
                    : sdHexPrism(q, vec2(0.29, hh));
}

// info: (material 1 box / 2 hex / 3 torus / 4 floor, id.x, id.y, half-height)
float map(vec3 p, out vec4 info){
    float d = p.y;                                        // floor
    info = vec4(4.0, 0.0, 0.0, 0.0);

    // --- array: 2x2 nearest-cell repetition, so a spinning object stays a
    //     correct SDF right across the cell boundary ---
    vec2 base = floor(p.xz / CELL - 0.5);
    for (int j = 0; j < 2; j++)
    for (int i = 0; i < 2; i++){
        vec2 id = base + vec2(i, j);
        float hh, cb;
        float dc = column(p, id, hh, cb);
        if (dc < d){ d = dc; info = vec4(cb < 0.5 ? 1.0 : 2.0, id, hh); }
    }
    // a column in a cell OUTSIDE the 2x2 block can still be nearer than the four
    // sampled ones (tall column beside a short one, ray coming in high). Cap the
    // step by a lower bound on that distance so the march cannot tunnel into it.
    {
        vec2 lo = p.xz - base * CELL, hi = (base + 2.0) * CELL - p.xz;
        float edge = min(min(lo.x, lo.y), min(hi.x, hi.y)) + 0.08;   // block edge + cell margin
        float cap = max(edge, max(p.y - 1.55, length(p.xz) - (ARRAYR + 1.0) * CELL));
        d = min(d, cap);
    }

    // --- ring of tori orbiting above the array ---
    vec3 rp = p - vec3(0.0, 2.05, 0.0);
    float a  = atan(rp.z, rp.x) - gPhase;                 // orbit: 1 turn per loop
    float sec = TAU / NRING;
    float i0 = floor(a / sec);
    float rr = length(rp.xz);
    for (int k = 0; k < 2; k++){                          // two nearest sectors -> continuous
        float i  = i0 + float(k);
        float an = i * sec;
        vec3 tq = vec3(rr * cos(a - an), rp.y, rr * sin(a - an)) - vec3(4.3, 0.0, 0.0);
        float ii = mod(i, NRING);
        tq.y -= 0.30 * sin(2.0 * gPhase + ii * TAU / NRING * 2.0);   // bob, wave round ring
        tq.yz *= rot(3.0 * gPhase + ii * TAU / NRING);              // tumble, 3 turns per loop
        tq.xy *= rot(0.55);                                          // fixed tilt: never a pure edge-on line
        float dt = sdTorus(tq, vec2(0.46, 0.15));
        if (dt < d){ d = dt; info = vec4(3.0, ii, 0.0, 0.0); }
    }
    return d;
}

float mapD(vec3 p){ vec4 i; return map(p, i); }

vec3 normal(vec3 p){
    vec2 e = vec2(0.0015, 0.0);
    return normalize(vec3(mapD(p + e.xyy) - mapD(p - e.xyy),
                          mapD(p + e.yxy) - mapD(p - e.yxy),
                          mapD(p + e.yyx) - mapD(p - e.yyx)));
}

float ao(vec3 p, vec3 n){
    float occ = 0.0, sca = 1.0;
    for (int i = 0; i < 5; i++){
        float h = 0.03 + 0.14 * float(i);
        occ += (h - mapD(p + n * h)) * sca;
        sca *= 0.75;
    }
    return clamp(1.0 - 1.5 * occ, 0.0, 1.0);
}

float softShadow(vec3 ro, vec3 rd, float k){
    float res = 1.0, t = 0.04;
    for (int i = 0; i < 24; i++){
        float h = mapD(ro + rd * t);
        res = min(res, k * h / t);
        t += clamp(h, 0.03, 0.35);
        if (res < 0.01 || t > 8.0) break;
    }
    return clamp(res, 0.0, 1.0);
}

float march(vec3 ro, vec3 rd, int steps, out vec4 info){
    float t = 0.0;
    for (int i = 0; i < STEPS; i++){
        if (i >= steps) break;
        vec4 inf;
        float d = map(ro + rd * t, inf);
        if (d < SURF * (1.0 + t * 0.5)){ info = inf; return t; }
        t += d * 0.9;
        if (t > MAXD) break;
    }
    info = vec4(0.0);
    return -1.0;
}

vec3 background(vec2 uv){
    vec3 bg = mix(vec3(0.020, 0.014, 0.045), vec3(0.075, 0.035, 0.125), uv.y + 0.5);
    // practicals stay TIGHT or they flood the frame and kill the silhouette
    bg += 0.55 * palette(0.02 + gDrift) * exp(-7.0 * length(uv - vec2(-0.70, 0.34)));
    bg += 0.45 * palette(0.55 + gDrift) * exp(-7.5 * length(uv - vec2( 0.72, 0.30)));
    return bg;
}

// full = key shadow + AO (primary hits); cheap = neither (reflection hits)
vec3 shade(vec3 p, vec3 n, vec3 v, vec4 info, bool full){
    float mat = info.x;
    vec3 key  = normalize(vec3(-0.55, 0.85, -0.45));
    vec3 fill = normalize(vec3( 0.85, 0.25,  0.45));
    float sh   = full ? softShadow(p + n * 0.01, key, 12.0) : 1.0;
    float occ  = full ? ao(p, n) : 0.8;
    float dif  = clamp(dot(n, key),  0.0, 1.0) * sh;
    float dif2 = clamp(dot(n, fill), 0.0, 1.0);
    float fres = pow(1.0 - clamp(dot(n, v), 0.0, 1.0), 4.0);
    float spec = pow(clamp(dot(reflect(-key,  n), v), 0.0, 1.0), 80.0) * sh;
    float shn  = pow(clamp(dot(reflect(-fill, n), v), 0.0, 1.0), 12.0);

    float hue, gloss = 0.0; vec3 base; vec3 emis = vec3(0.0);
    if (mat < 2.5){
        // per-instance hue: radial ring index + checkerboard contrast + global drift
        float r  = length(info.yz * CELL);
        hue = 0.09 * r + 0.28 * (mat - 1.0) + gDrift;
        // height ramp: dark feet -> bright caps
        float hr = smoothstep(0.0, 2.0 * info.w, p.y);
        base = palette(hue) * mix(0.40, 1.05, hr);
        base = mix(base, palette(hue + 0.12), 0.35 * hr);
        // crest cells glow on the cap: the wave reads as light, not just height
        emis = palette(hue + 0.05) * 1.6 * smoothstep(0.56, 0.72, info.w) * smoothstep(0.7, 0.95, n.y);
    } else if (mat < 3.5){
        hue = info.y / NRING + 0.5 + gDrift;
        base = palette(hue) * 0.95;
        gloss = 0.6;
    } else {
        // floor: near-black, a faint coloured pool under the array; the gloss
        // comes from the reflection bounce, not from lighting terms — the rim /
        // fill terms flood a plane at grazing angles and drown the silhouette
        hue = 0.5 + gDrift;
        float pool = exp(-0.05 * dot(p.xz, p.xz));
        base = vec3(0.030, 0.026, 0.045) + 0.06 * palette(hue) * pool;
        return base * (0.4 + 0.8 * dif) * occ + vec3(1.0) * spec * 0.6;
    }

    vec3 col = base * (0.22 + 2.30 * dif) * occ;
    col += base * palette(hue + 0.5) * 0.35 * occ;            // tinted ambient
    col += palette(hue + 0.55) * dif2 * 0.60 * occ;           // contrasting fill
    col += emis;
    col += vec3(1.0) * spec * (1.0 + 1.6 * gloss);            // hotspot
    col += palette(hue + 0.30) * shn * 0.40 * (1.0 - gloss * 0.5) * occ;
    col += palette(hue + 0.50) * fres * 1.10 * (1.0 - 0.5 * gloss) * occ;   // contrasting rim
    return col;
}

void mainImage(out vec4 fragColor, in vec2 fragCoord){
    vec2 uv = (fragCoord - 0.5 * iResolution.xy) / iResolution.y;
    // fract() first: sin(TAU) is not bit-equal to sin(0) in float, and
    // fract(h + 1.0) loses low bits vs fract(h). Wrapping the time before the
    // multiply is what makes t=0 and t=LOOP land on the same bits.
    float lt = fract(iTime / LOOP);
    gPhase  = TAU * lt;
    gDrift  = lt;

    // ---- camera: elevated, drifting on its own slow orbit ----
    float yaw = 0.28 * sin(gPhase);
    float pit = 0.46 + 0.05 * cos(gPhase);
    float dol = 13.5 + 0.7 * sin(gPhase + 1.3);
    if (iMouse.z > 0.5){
        yaw = (iMouse.x / iResolution.x - 0.5) * 6.0;
        pit = 0.15 + (iMouse.y / iResolution.y) * 1.2;
    }
    vec3 ta = vec3(0.4 * sin(gPhase), 0.35, 0.0);
    vec3 ro = ta + vec3(sin(yaw) * cos(pit), sin(pit), -cos(yaw) * cos(pit)) * dol;
    vec3 fw = normalize(ta - ro), rt = normalize(cross(vec3(0,1,0), fw)), up = cross(fw, rt);
    vec3 rd = normalize(uv.x * rt + uv.y * up + 1.75 * fw);

    vec3 bg  = background(uv);
    vec3 col = bg;

    vec4 info;
    float t = march(ro, rd, STEPS, info);
    if (t > 0.0){
        vec3 p = ro + rd * t;
        vec3 n = normal(p);
        col = shade(p, n, -rd, info, true);

        // glossy floor: one reflection bounce, cheap shading
        if (info.x > 3.5){
            vec3 rr = reflect(rd, n);
            vec4 rinfo;
            float rt2 = march(p + n * 0.02, rr, RSTEPS, rinfo);
            vec3 rcol = bg * 0.25;
            if (rt2 > 0.0){
                vec3 rp = p + n * 0.02 + rr * rt2;
                rcol = shade(rp, normal(rp), -rr, rinfo, false) * exp(-0.08 * rt2);
            }
            float fr = 0.12 + 0.60 * pow(1.0 - clamp(dot(n, -rd), 0.0, 1.0), 3.0);
            col += rcol * fr;
        }
        col = mix(bg, col, exp(-0.0010 * t * t));               // depth haze
    }

    // ---- grade (no gamma pass: palette is display-ready sRGB) ----
    col *= 1.15;
    col  = mix(vec3(dot(col, vec3(0.299, 0.587, 0.114))), col, 1.18);
    col  = col / (1.0 + col * 0.45);
    col *= 1.0 - 0.40 * dot(uv, uv);
    col  = clamp(col, 0.0, 1.0);
    col += (hash13(vec3(fragCoord, 1.0)) - 0.5) * 0.012;            // static dither, loop-safe
    fragColor = vec4(col, 1.0);
}