#version 300 es
precision highp float;

// This bar's framebuffer size, in pixels.
uniform vec2  u_res;
// This bar's bottom-left corner within the output, in pixels.
uniform vec2  u_origin;
// Full output size, in pixels.
uniform vec2  u_output;
// Animation phase in radians, 0..TAU. Wrapping this on the CPU is exact because
// every time term below is an integer multiple of it.
uniform float u_phase;
uniform float u_brightness;
// Inner fade width, as a fraction of bar width.
uniform float u_fade;
// +1.0 when the content-facing edge is this bar's right edge, -1.0 when it's the left.
uniform float u_inner_dir;

uniform vec3 u_c0;
uniform vec3 u_c1;
uniform vec3 u_c2;

out vec4 frag;

void main() {
    // Normalise against output height so both bars sample one continuous field
    // and the pattern reads as a single image wrapping around the content.
    vec2 g = (u_origin + gl_FragCoord.xy) / u_output.y;
    float p = u_phase;

    // Domain warp: two cheap sine fields displace the sample point, which is what
    // turns plain sine bands into something that looks like drifting cloud.
    float wx = sin(g.y * 3.1 + p * 2.0) + sin(g.x * 2.2 - p * 3.0);
    float wy = sin(g.x * 2.7 - p * 1.0) + sin(g.y * 1.9 + p * 2.0);
    vec2 q = g + 0.30 * vec2(wx, wy);

    float a = sin(q.x * 2.4 + p * 1.0) * 0.5 + 0.5;
    float b = sin(q.y * 1.7 - p * 2.0) * 0.5 + 0.5;
    float c = sin((q.x + q.y) * 1.1 + p * 3.0) * 0.5 + 0.5;
    float m = clamp(a * 0.40 + b * 0.35 + c * 0.25, 0.0, 1.0);

    vec3 col = mix(u_c0, u_c1, smoothstep(0.00, 0.62, m));
    col = mix(col, u_c2, smoothstep(0.55, 1.00, m));

    // Fade out towards the edge that touches the content, so the bars don't draw
    // a hard bright frame around whatever you're actually watching.
    float xn = gl_FragCoord.x / u_res.x;
    float d = u_inner_dir > 0.0 ? (1.0 - xn) : xn;
    float edge = u_fade > 0.0 ? smoothstep(0.0, u_fade, d) : 1.0;

    col *= u_brightness * edge;

    // At these brightness levels 8-bit quantisation bands visibly, and a static
    // band is exactly the kind of fixed pattern this whole program exists to avoid.
    float n = fract(sin(dot(gl_FragCoord.xy, vec2(12.9898, 78.233)) + p) * 43758.5453);
    col += (n - 0.5) * (1.0 / 255.0);

    // Wayland wants premultiplied alpha; col is already scaled by `edge`.
    frag = vec4(max(col, vec3(0.0)), edge);
}
