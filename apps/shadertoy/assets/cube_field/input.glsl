// Cube-field scene from https://www.shadertoy.com/view/tssSDN
// Reference: https://qiita.com/ukeyshima/items/221b0384d39f521cad8f
// TRUEOS: analytic sphere + bounded grid traversal of the same animated boxes.
#define REP .04
#define FAR 1000.
#define PI2 (3.141592 * 2.)

float rand(vec2 co) {
    return fract(sin(dot(co, vec2(12.9898, 78.233))) * 43758.5453);
}

float columnTop(vec2 cell, vec3 sphere) {
    float hash = rand(cell * .001);
    vec2 center = cell * REP + REP * .5;
    float bsDist = length(sphere.xz - center);
    float s = smoothstep(0., .5, bsDist);
    // Same center height and .1 box half-height as the source scene.
    return .125 - sin(hash * PI2 + iTime * (2. + bsDist * .015))
        * .05 * (1. - pow(s, .9)) + .1;
}

float sphereHit(vec3 ro, vec3 rd, vec3 center) {
    vec3 oc = ro - center;
    float b = dot(oc, rd);
    float c = dot(oc, oc) - .075 * .075;
    float h = b * b - c;
    if (h < 0.) return FAR;
    float t = -b - sqrt(h);
    return t >= 0. ? t : FAR;
}

float columnHit(vec3 ro, vec3 rd, vec3 sphere, float limit, out vec3 normal) {
    normal = vec3(0., 1., 0.);
    // The fixed camera looks down onto boxes whose tops lie in [.175, .275].
    if (rd.y >= -1e-6) return FAR;
    // Outside the sphere's .5-radius influence the touching boxes form one
    // exact flat plane. Intersect it directly and visit only affected cells.
    float plane = (.225 - ro.y) / rd.y;
    vec2 planeCell = floor((ro + rd * plane).xz / REP) * REP + REP * .5;
    float flatHit = length(planeCell - sphere.xz) >= .5 ? plane : FAR;
    vec2 direction = mix(vec2(-1.), vec2(1.), step(vec2(0.), rd.xz));
    vec2 invAbs = 1. / max(abs(rd.xz), vec2(1e-8));
    vec2 nearBox = (sphere.xz - vec2(.521) - ro.xz) * direction * invAbs;
    vec2 farBox = (sphere.xz + vec2(.521) - ro.xz) * direction * invAbs;
    vec2 nearSlab = min(nearBox, farBox);
    vec2 farSlab = max(nearBox, farBox);
    float begin = max(max(0., (.275 - ro.y) / rd.y), max(nearSlab.x, nearSlab.y));
    float end = min(min((.175 - ro.y) / rd.y, min(limit, flatHit)), min(farSlab.x, farSlab.y));
    if (begin > end) return flatHit;
    vec3 start = ro + rd * (begin + 1e-6);
    vec2 cell = floor(start.xz / REP);
    vec2 boundary = (cell + step(vec2(0.), rd.xz)) * REP;
    vec2 next = (boundary - ro.xz) * direction * invAbs;
    vec2 delta = REP * invAbs;
    float entry = begin;
    vec3 entryNormal = vec3(0., 1., 0.);
    // Bound work by cell crossings inside the height slab, not empty air.
    int cells = int(ceil((end - begin) * (abs(rd.x) + abs(rd.z)) / REP)) + 3;
    cells = min(cells, 128);
    for (int i = 0; i < cells; i++) {
        float exit = min(min(next.x, next.y), end);
        float top = (columnTop(cell, sphere) - ro.y) / rd.y;
        float hit = max(entry, top);
        if (hit <= exit) {
            normal = top >= entry ? vec3(0., 1., 0.) : entryNormal;
            return hit;
        }
        if (exit >= end) break;
        if (next.x < next.y) {
            entry = next.x;
            next.x += delta.x;
            cell.x += direction.x;
            entryNormal = vec3(-direction.x, 0., 0.);
        } else {
            entry = next.y;
            next.y += delta.y;
            cell.y += direction.y;
            entryNormal = vec3(0., 0., -direction.y);
        }
    }
    return flatHit;
}

vec3 getRayDir(vec2 uv, vec3 p, vec3 l, float z) {
    vec3 forward = normalize(l - p);
    vec3 right = normalize(cross(forward, vec3(0., 1., 0.)));
    vec3 up = normalize(cross(right, forward));
    return normalize(right * uv.x + up * uv.y + forward * z);
}

void mainImage(out vec4 fragColor, in vec2 fragCoord) {
    vec2 uv = (fragCoord.xy * 2. - iResolution.xy)
        / min(iResolution.x, iResolution.y);
    vec3 ro = vec3(1., 1., 1.2);
    vec3 rd = getRayDir(uv, ro, vec3(0., .2, 0.), 3.5);
    vec3 sphere = vec3(sin(iTime * 1.8) * .25, .32, cos(iTime * 2.2) * .3);
    float ball = sphereHit(ro, rd, sphere);
    vec3 n;
    float boxes = columnHit(ro, rd, sphere, ball, n);
    float dist = min(ball, boxes);
    vec3 col = vec3(0.);
    if (dist < FAR) {
        if (ball < boxes) n = normalize(ro + rd * ball - sphere);
        vec3 l = normalize(vec3(1., 1., -1.));
        float diffuse = dot(l, n) * .5 + .5;
        col = vec3(diffuse);
        if (ball < boxes) {
            col *= vec3(1., 0., 0.);
        } else {
            if (n.x > .5) col = diffuse * vec3(1., 0., 0.);
            if (n.y > .5) col = diffuse * vec3(1., .9, .9);
            if (n.z > .5) col = diffuse * vec3(.6, 0., 0.);
        }
    }
    fragColor = vec4(pow(col, vec3(.4545)), 1.);
}
