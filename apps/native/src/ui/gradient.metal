// The gradient backdrops (`gradients.rs`): a slow mesh of three colours over a page's ground, drawn straight into the
// two planes of a 4:2:0 YCbCr frame (luma, then chroma at half size), which GPUI draws as a surface, or as one BGRA
// picture into a layer under the window (`gradient_rgb`), with the pixel backdrops' cells (`cells_rgb`). Wings' own code;
// the looks recreate shadergradient's plane, sphere and waterPlane meshes on a flat picture.

#include <metal_stdlib>
using namespace metal;

// As `Uniforms` in gradients.rs.
struct Gradient {
  float4 ink1;
  float4 ink2;
  float4 ink3;
  float4 ground;
  float2 size;
  float2 target;
  float time;
  float shape;
  float density;
  float strength;
  float twist;
  float grain;
  float corner;
  float luma_max;
  float luma_min;
  float ground_luma;
  float2 pad;
};

// As `LOOP` in gradients.rs: the time comes back to 0 here, and every slow wave turns a whole number of times in it.
constant float LOOP = 64.0;
constant float TURN = 6.28318530718 / LOOP;

struct Vertex {
  float4 position [[position]];
};

// One triangle over the whole target.
vertex Vertex gradient_vertex(uint id [[vertex_id]]) {
  float2 corner = float2(float((id << 1) & 2), float(id & 2));
  return Vertex{float4(corner * 2.0 - 1.0, 0.0, 1.0)};
}

float hash(float3 p) {
  p = fract(p * float3(0.1127, 0.1283, 0.0917));
  p += dot(p, p.zyx + 29.71);
  return fract((p.x + p.z) * p.y);
}

// Smooth value noise, -1 to 1.
float noise(float3 p) {
  float3 i = floor(p);
  float3 f = fract(p);
  float3 u = f * f * (3.0 - 2.0 * f);
  float a = mix(hash(i), hash(i + float3(1, 0, 0)), u.x);
  float b = mix(hash(i + float3(0, 1, 0)), hash(i + float3(1, 1, 0)), u.x);
  float c = mix(hash(i + float3(0, 0, 1)), hash(i + float3(1, 0, 1)), u.x);
  float d = mix(hash(i + float3(0, 1, 1)), hash(i + float3(1, 1, 1)), u.x);
  return mix(mix(a, b, u.y), mix(c, d, u.y), u.z) * 2.0 - 1.0;
}

float layered(float3 p) {
  return 0.7 * noise(p) + 0.3 * noise(p * 2.03 + float3(5.2, 1.3, 7.7));
}

// Noise that drifts with time and is where it started once the time comes round: the field now, faded into the field a
// loop earlier, kept as strong at the middle of the fade as at its ends.
float drifting(float2 q, float t) {
  float w = t / LOOP;
  float now = layered(float3(q, t));
  float before = layered(float3(q, t - LOOP));
  return mix(now, before, w) * rsqrt(w * w + (1.0 - w) * (1.0 - w));
}

// Broad folds, one colour across them and the third on their crests, lit from the top left.
float3 plane(float2 q, float t, constant Gradient &g) {
  float2 p = q * g.density * 1.6;
  float h = drifting(p, t);
  float e = 0.04;
  float hx = drifting(p + float2(e, 0.0), t);
  float hy = drifting(p + float2(0.0, e), t);
  float3 normal = normalize(float3(-(hx - h) / e * 0.35, -(hy - h) / e * 0.35, 1.0));
  float light = dot(normal, normalize(float3(-0.45, -0.55, 0.7)));
  float across = smoothstep(-0.9, 0.9, q.x * 1.4 + h * 0.5);
  float3 color = mix(g.ink1.rgb, g.ink2.rgb, across);
  color = mix(color, g.ink3.rgb, smoothstep(-0.2, 0.9, h) * g.strength);
  return color * (0.78 + 0.32 * light);
}

// smoothstep's edges always go low to high: Metal leaves it undefined the other way round.
// A round bloom drifting about the middle, twisting, two colours top to bottom inside it and the third at its rim.
float3 sphere(float2 q, float t, constant Gradient &g) {
  float2 centre = float2(0.16 * sin(t * TURN), 0.09 * cos(t * TURN * 2.0));
  float2 d = q - centre;
  float r = length(d);
  float swirl = sin(d.y * 5.5 + t * TURN * 6.0) * g.twist * 0.6 * (1.0 - smoothstep(0.0, 0.7, r));
  float2 turned = float2(d.x * cos(swirl) - d.y * sin(swirl), d.x * sin(swirl) + d.y * cos(swirl));
  float edge = 0.40 + 0.07 * drifting(normalize(d + 1e-4) * g.density * 1.4, t);
  float body = (1.0 - smoothstep(edge * 0.2, edge, r));
  float depth = sqrt(saturate(1.0 - (r / edge) * (r / edge)));
  float3 normal = normalize(float3(turned / edge, depth + 0.2));
  float light = dot(normal, normalize(float3(-0.4, -0.6, 0.7)));
  float3 inside = mix(g.ink2.rgb, g.ink1.rgb, (1.0 - smoothstep(-0.3, 0.3, turned.y + 0.1 * drifting(turned * 2.0, t))));
  inside *= 0.75 + 0.35 * light;
  float off_rim = (r - edge * 0.92) / 0.07;
  float rim = exp(-off_rim * off_rim);
  float3 haze = mix(g.ground.rgb, g.ink3.rgb, 0.45 * (1.0 - smoothstep(0.25, 1.1, r)));
  float3 color = mix(haze, inside, body);
  return mix(color, g.ink3.rgb, rim * g.strength);
}

// Ripples crossing each other on a surface that drifts, the third colour on their crests.
float3 water(float2 q, float t, constant Gradient &g) {
  float2 p = q * g.density * 2.4;
  float warp = drifting(p * 0.45, t);
  float waves = sin(p.x * 3.1 + p.y * 1.2 + warp * 2.4 + t * TURN * 6.0) +
                0.7 * sin(-p.x * 1.6 + p.y * 2.8 + warp * 1.7 - t * TURN * 5.0) +
                0.4 * sin(p.y * 4.2 - p.x * 0.7 + t * TURN * 8.0);
  float h = waves / 2.1;
  float3 color = mix(g.ink1.rgb, g.ink2.rgb, smoothstep(-0.8, 0.8, q.x * 1.3 + warp * 0.45));
  color = mix(color, g.ink3.rgb, smoothstep(0.25, 0.95, h) * g.strength);
  return color * (0.86 + 0.14 * (h * 0.5 + 0.5));
}

// Thin antialiased lines where `v` crosses a whole number, about `width` pixels across. Lines closer than a couple of pixels
// fade out, as they would shimmer otherwise.
float line(float v, float width) {
  float px = max(fwidth(v), 1e-5);
  float d = abs(fract(v + 0.5) - 0.5) / px;
  return saturate(width * 0.5 - d + 0.5) * (1.0 - smoothstep(0.3, 0.5, px));
}

float2 hash2(float2 cell) {
  return float2(hash(float3(cell, 1.7)), hash(float3(cell, 8.3)));
}

// A map's contour lines of a hill field that drifts: thin ones every level, a thicker one every fourth.
float3 topo(float2 q, float t, constant Gradient &g) {
  float2 p = q * g.density * 2.2;
  float h = drifting(p, t);
  float v = h * 12.0 * g.twist;
  float3 tint = mix(g.ink1.rgb, g.ink2.rgb, smoothstep(-0.5, 0.5, h));
  float3 color = mix(g.ground.rgb, tint, 0.4);
  float lines = max(line(v, 1.1) * 0.55, line(v * 0.25, 1.9));
  return mix(color, g.ink3.rgb, saturate(lines) * g.strength);
}

// Parallel lines across the page that bend and thicken with a drifting field.
float3 flow(float2 q, float t, constant Gradient &g) {
  float lines = g.density * 30.0;
  float w = drifting(float2(q.x * 1.3, q.y * 1.1) * 1.6, t);
  float bend = g.twist * (0.10 * w + 0.025 * sin(q.x * 4.0 + t * TURN * 4.0));
  float thick = 1.0 + 1.4 * smoothstep(-0.2, 0.6, drifting(q * 2.4 + 20.0, t));
  float l = line((q.y + bend) * lines, thick);
  float3 tint = mix(g.ink1.rgb, g.ink2.rgb, smoothstep(-0.9, 0.9, q.x + 0.5 * w));
  return mix(g.ground.rgb, tint, l * g.strength);
}

// A floor of lines in perspective running away to a glowing horizon, a pulse of light travelling along it.
float3 grid(float2 q, float t, constant Gradient &g) {
  float dy = q.y + 0.10;
  float3 color = mix(g.ground.rgb, g.ink2.rgb, 0.5 * exp(-abs(dy) * 5.0));
  float horizon = 1.0 - smoothstep(0.0, 1.6 / g.size.y, abs(dy));
  if (dy > 0.002) {
    float z = 1.0 / dy;
    float across = line(q.x * z * g.density * 5.0, 1.0);
    float along = line(z * g.density * 0.9 + t / LOOP * 48.0, 1.0);
    float pulse = pow(0.5 + 0.5 * sin(6.28318530718 * (z * 0.12 + t / LOOP * 4.0)), 5.0);
    float lit = max(across, along) * (0.4 + 0.6 * pulse) * smoothstep(0.0, 0.25, dy);
    color = mix(color, g.ink1.rgb, lit * g.strength);
  }
  return mix(color, g.ink3.rgb, horizon * 0.8 * g.strength);
}

// A matrix of dots whose sizes swell with a drifting field and a ring that travels out from a point.
float3 dots(float2 q, float t, constant Gradient &g) {
  float cells = g.density * 46.0;
  float2 p = q * cells;
  float2 f = fract(p) - 0.5;
  float2 c = (floor(p) + 0.5) / cells;
  float2 orb = float2(0.35 * sin(t * TURN), 0.18 * cos(t * TURN * 2.0));
  float ring = 0.5 + 0.5 * sin(length(c - orb) * 11.0 - t * TURN * 8.0);
  float level = saturate(0.5 + 0.9 * drifting(c * 1.7, t)) * (0.55 + 0.45 * ring);
  float r = 0.06 + 0.40 * level;
  float aa = fwidth(p.x);
  float dot_ = 1.0 - smoothstep(r - aa, r + aa, length(f));
  float3 tint = mix(g.ink1.rgb, g.ink2.rgb, smoothstep(0.35, 0.8, level));
  return mix(g.ground.rgb, tint, dot_ * g.strength);
}

// Folds of silk: warped bands of two colours, shaded, with a thin bright line along each crest.
float3 silk(float2 q, float t, constant Gradient &g) {
  float2 p = q * g.density * float2(0.9, 1.4);
  float w1 = drifting(p * 0.8 + float2(3.1, 7.4), t);
  float w2 = drifting(p * 0.8 + float2(11.7, 2.3), t);
  float f = p.y * 3.2 + 1.8 * w1 + 0.9 * w2 + p.x * 0.5;
  float band = 0.5 + 0.5 * sin(f * 7.0 + t * TURN * 2.0);
  float3 color = mix(g.ink1.rgb, g.ink2.rgb, smoothstep(0.15, 0.85, 0.5 + 0.5 * sin(f * 1.1 + w2 * 2.0)));
  color *= 0.78 + 0.34 * band;
  float off = (band - 0.93) / 0.05;
  float crest = exp(-off * off);
  return mix(mix(g.ground.rgb, color, 0.9), g.ink3.rgb, crest * 0.8 * g.strength);
}

// Voronoi cells around points that circle slowly: a tint to each, thin walls between them and a pin at each middle.
float3 voronoi(float2 q, float t, constant Gradient &g) {
  float2 p = q * g.density * 5.0;
  float2 i = floor(p);
  float2 f = fract(p);
  float d1 = 8.0;
  float d2 = 8.0;
  float id = 0.0;
  for (int y = -1; y <= 1; y++) {
    for (int x = -1; x <= 1; x++) {
      float2 n = float2(x, y);
      float2 h = hash2(i + n);
      float2 point = n + 0.5 + 0.38 * sin(t * TURN * 3.0 + 6.28318530718 * h + float2(0.0, 1.5708));
      float d = length(point - f);
      if (d < d1) {
        d2 = d1;
        d1 = d;
        id = hash(float3(i + n, 4.1));
      } else if (d < d2) {
        d2 = d;
      }
    }
  }
  float aa = fwidth(p.x);
  float wall = saturate(0.6 - 0.5 * (d2 - d1) / aa + 0.5);
  float pin = 1.0 - smoothstep(0.03, 0.03 + aa * 1.5, d1);
  float3 color = mix(g.ink1.rgb, g.ink2.rgb, id) * (0.85 + 0.2 * (1.0 - d1));
  color = mix(g.ground.rgb, color, 0.85);
  color = mix(color, g.ink3.rgb, wall * g.strength);
  return mix(color, g.ink3.rgb, pin * 0.6 * g.strength);
}

// Two sets of lines a few degrees apart that turn slowly against each other: the fringes between them are the picture.
float3 moire(float2 q, float t, constant Gradient &g) {
  float a = 0.09 + 0.06 * sin(t * TURN * 2.0);
  float freq = g.density * 40.0;
  float v1 = (q.x * cos(a) + q.y * sin(a)) * freq;
  float v2 = (q.x * cos(a) - q.y * sin(a)) * freq * 1.02;
  float m1 = line(v1, 2.2);
  float m2 = line(v2, 2.2);
  float3 color = mix(g.ground.rgb, g.ink1.rgb, m1 * 0.8 * g.strength);
  color = mix(color, g.ink2.rgb, m2 * 0.8 * g.strength);
  return mix(color, g.ink3.rgb, m1 * m2 * g.strength);
}

float3 to_linear(float3 c) {
  return select(pow((c + 0.055) / 1.055, 2.4), c / 12.92, c <= 0.04045);
}

float3 to_srgb(float3 c) {
  return select(1.055 * pow(c, 1.0 / 2.4) - 0.055, c * 12.92, c <= 0.0031308);
}

// The pixel at `at` in the frame's pixels: the mesh, faded to the ground in the rounded corners of what holds it (where
// GPUI can't clip a surface round), grain, and then held within the luminance muted text stays readable on.
float3 shade(float2 at, constant Gradient &g) {
  float2 uv = at / g.size;
  float2 q = (uv - 0.5) * float2(g.size.x / g.size.y, 1.0);
  float3 color;
  if (g.shape < 0.5) {
    color = plane(q, g.time, g);
  } else if (g.shape < 1.5) {
    color = sphere(q, g.time, g);
  } else if (g.shape < 2.5) {
    color = water(q, g.time, g);
  } else if (g.shape < 3.5) {
    color = topo(q, g.time, g);
  } else if (g.shape < 4.5) {
    color = flow(q, g.time, g);
  } else if (g.shape < 5.5) {
    color = grid(q, g.time, g);
  } else if (g.shape < 6.5) {
    color = dots(q, g.time, g);
  } else if (g.shape < 7.5) {
    color = silk(q, g.time, g);
  } else if (g.shape < 8.5) {
    color = voronoi(q, g.time, g);
  } else {
    color = moire(q, g.time, g);
  }
  float corner = max(g.corner, 1.0);
  float2 from_corner = abs(at - g.size * 0.5) - (g.size * 0.5 - corner);
  float into_corner = length(max(from_corner, 0.0));
  color = mix(color, g.ground.rgb, smoothstep(corner * 0.45, corner, into_corner));
  color += (hash(float3(floor(at), 3.0)) - 0.5) * g.grain * 0.05;

  float3 lin = to_linear(saturate(color));
  float3 ground = to_linear(g.ground.rgb);
  float luma = dot(lin, float3(0.2126, 0.7152, 0.0722));
  // Luminance is linear in linear light, so a move towards the ground lands exactly on the bound.
  if (luma > g.luma_max) {
    lin = ground + (lin - ground) * (max(g.luma_max - g.ground_luma, 0.0) / max(luma - g.ground_luma, 1e-6));
  } else if (luma < g.luma_min) {
    lin = ground + (lin - ground) * (max(g.ground_luma - g.luma_min, 0.0) / max(g.ground_luma - luma, 1e-6));
  }
  return to_srgb(lin);
}

// GPUI's surface shader reads BT.601 full range (`surface_fragment` in gpui's shaders.metal).
float luma_of(float3 c) {
  return dot(c, float3(0.299, 0.587, 0.114));
}

fragment float4 gradient_luma(Vertex in [[stage_in]], constant Gradient &g [[buffer(0)]]) {
  float2 at = in.position.xy * (g.size / g.target);
  float3 color = shade(at, g);
  // A half step of dither, so the dark ramps don't band in 8 bits.
  float dither = (hash(float3(in.position.xy, 11.0)) - 0.5) / 255.0;
  return float4(luma_of(color) + dither, 0.0, 0.0, 1.0);
}

fragment float4 gradient_chroma(Vertex in [[stage_in]], constant Gradient &g [[buffer(0)]]) {
  float2 at = in.position.xy * (g.size / g.target);
  float3 color = shade(at, g);
  float y = luma_of(color);
  return float4((color.b - y) / 1.772 + 0.5, (color.r - y) / 1.402 + 0.5, 0.0, 1.0);
}

// The frame as one opaque BGRA picture, for the layer Wings draws a backdrop in under the window (`native_backdrop.rs`):
// the same colour, untagged as GPUI's own drawing is, which writes sRGB values as they are.
fragment float4 gradient_rgb(Vertex in [[stage_in]], constant Gradient &g [[buffer(0)]]) {
  float2 at = in.position.xy * (g.size / g.target);
  float3 color = shade(at, g);
  float dither = (hash(float3(in.position.xy, 11.0)) - 0.5) / 255.0;
  return float4(saturate(color + dither), 1.0);
}

// As `CellColors` in native_backdrop.rs: the size of the grid, and each shade's colour over the ground (`pixels.rs`).
struct Cells {
  uint cols;
  uint rows;
  uint2 pad;
  float4 colors[7];
};

// The pixel backdrops, one pixel a cell, which the layer scales up without smoothing: each cell's shade picks its colour.
fragment float4 cells_rgb(Vertex in [[stage_in]], constant Cells &c [[buffer(0)]],
                          device const uchar *shades [[buffer(1)]]) {
  uint2 at = min(uint2(in.position.xy), uint2(c.cols - 1, c.rows - 1));
  return c.colors[min(uint(shades[at.y * c.cols + at.x]), 6u)];
}
