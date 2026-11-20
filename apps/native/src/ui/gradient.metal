// The gradient backdrops (`gradients.rs`): a slow mesh of three colours over a page's ground, drawn straight into the
// two planes of a 4:2:0 YCbCr frame (luma, then chroma at half size), which GPUI draws as a surface. Wings' own code; the
// looks recreate shadergradient's plane, sphere and waterPlane meshes on a flat picture.

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
  } else {
    color = water(q, g.time, g);
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
