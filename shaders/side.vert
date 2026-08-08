#version 300 es
// Fullscreen triangle generated from gl_VertexID - no vertex buffers, no VAO
// state, nothing to upload per frame.
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
