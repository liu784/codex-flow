import type { ContinuousPose } from './cat-controller';

// Small relief mesh, NOT reconstructed 3D. Depth is a hand-tuned smooth face /
// muzzle approximation. It cannot reveal unseen fur as a real model would.
const vertexSource = `
attribute vec2 uv;
uniform vec2 size;
uniform vec2 fit;
uniform vec3 angles;
uniform vec2 shift;
varying vec2 texCoord;
void main() {
  texCoord = uv;
  vec2 point = vec2((uv.x - 0.5) * size.x, (0.58 - uv.y) * size.y);
  vec2 face = (uv - vec2(0.5, 0.43)) / vec2(0.40, 0.36);
  float dome = sqrt(max(0.0, 1.0 - dot(face, face)));
  vec2 muzzle = (uv - vec2(0.5, 0.48)) / vec2(0.12, 0.10);
  float depth = size.x * (0.19 * dome + 0.07 * exp(-dot(muzzle, muzzle)));
  float cy = cos(angles.x), sy = sin(angles.x);
  float cp = cos(angles.y), sp = sin(angles.y);
  float x = cy * point.x + sy * depth;
  float z = -sy * point.x + cy * depth;
  float y = cp * point.y + sp * z;
  z = -sp * point.y + cp * z;
  vec2 rotated = vec2(x, y) * (800.0 / (800.0 - (z - depth)));
  float cr = cos(angles.z), sr = sin(angles.z);
  rotated = mat2(cr, -sr, sr, cr) * rotated + vec2(shift.x, -shift.y);
  // Feather motion to zero at the neck. Body and chest seam never translate.
  float weight = 1.0 - smoothstep(0.64, 0.91, uv.y);
  vec2 finalPoint = mix(point, rotated, weight);
  gl_Position = vec4(vec2(finalPoint.x / size.x * 2.0, finalPoint.y / size.y * 2.0 - 0.16) * fit, 0.0, 1.0);
}`;
const fragmentSource = `precision mediump float; varying vec2 texCoord; uniform sampler2D portrait;
void main() { gl_FragColor = texture2D(portrait, texCoord); }`;

export class CatHeadRenderer {
  private gl: WebGLRenderingContext;
  private program: WebGLProgram | null = null;
  private buffers: WebGLBuffer[] = [];
  private texture: WebGLTexture | null = null;
  private count = 0;
  private width = 1;
  private height = 1;
  private last = '';
  private lastPaint = -Infinity;
  private size: WebGLUniformLocation | null = null;
  private angles: WebGLUniformLocation | null = null;
  private shift: WebGLUniformLocation | null = null;
  private fit: WebGLUniformLocation | null = null;
  constructor(private canvas: HTMLCanvasElement, image: HTMLImageElement) {
    const gl = canvas.getContext('webgl', { alpha: true, premultipliedAlpha: false, antialias: true, depth: false });
    if (!gl) throw new Error('Head mesh WebGL unavailable');
    this.gl = gl;
    try {
      const shaders: WebGLShader[] = [];
      try {
        for (const [type, source] of [[gl.VERTEX_SHADER, vertexSource], [gl.FRAGMENT_SHADER, fragmentSource]] as const) {
          const shader = gl.createShader(type); if (!shader) throw new Error('Head shader allocation failed');
          shaders.push(shader); gl.shaderSource(shader, source); gl.compileShader(shader);
          if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(shader) ?? 'Head shader compile failed');
        }
        const program = gl.createProgram(); if (!program) throw new Error('Head program allocation failed');
        this.program = program; shaders.forEach(shader => gl.attachShader(program, shader)); gl.linkProgram(program);
        if (!gl.getProgramParameter(program, gl.LINK_STATUS)) throw new Error('Head program link failed');
      } finally { shaders.forEach(shader => gl.deleteShader(shader)); }
      gl.useProgram(this.program);
      const points: number[] = [], indices: number[] = [], cols = 40, rows = 48;
      for (let y = 0; y <= rows; y++) for (let x = 0; x <= cols; x++) points.push(x / cols, y / rows);
      for (let y = 0; y < rows; y++) for (let x = 0; x < cols; x++) {
        const p = y * (cols + 1) + x; indices.push(p, p + 1, p + cols + 1, p + 1, p + cols + 2, p + cols + 1);
      }
      for (const [target, data] of [[gl.ARRAY_BUFFER, new Float32Array(points)], [gl.ELEMENT_ARRAY_BUFFER, new Uint16Array(indices)]] as const) {
        const buffer = gl.createBuffer(); if (!buffer) throw new Error('Head buffer allocation failed');
        this.buffers.push(buffer); gl.bindBuffer(target, buffer); gl.bufferData(target, data, gl.STATIC_DRAW);
      }
      const uv = gl.getAttribLocation(this.program!, 'uv'); gl.enableVertexAttribArray(uv); gl.vertexAttribPointer(uv, 2, gl.FLOAT, false, 0, 0);
      this.count = indices.length;
      this.size = gl.getUniformLocation(this.program!, 'size'); this.angles = gl.getUniformLocation(this.program!, 'angles'); this.shift = gl.getUniformLocation(this.program!, 'shift');
      this.fit = gl.getUniformLocation(this.program!, 'fit');
      {
        const texture = gl.createTexture(); if (!texture) throw new Error('Head texture allocation failed');
        this.texture = texture; gl.bindTexture(gl.TEXTURE_2D, texture);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
        gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR); gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
        gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, image);
      }
      if (gl.getError() !== gl.NO_ERROR) throw new Error('Head mesh initialization failed');
    } catch (error) { this.dispose(); throw error; }
  }
  resize(width: number, height: number) {
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    this.width = Math.max(1, width); this.height = Math.max(1, height);
    this.canvas.width = Math.round(this.width * dpr); this.canvas.height = Math.round(this.height * dpr);
    this.gl.viewport(0, 0, this.canvas.width, this.canvas.height); this.last = '';
  }
  render(pose: ContinuousPose) {
    const key = `${pose.yaw.toFixed(3)}:${pose.pitch.toFixed(3)}`;
    if (key === this.last || this.gl.isContextLost()) return;
    const now = performance.now();
    if (now - this.lastPaint < 1000 / 60) return;
    this.lastPaint = now;
    this.last = key;
    const gl = this.gl, radians = Math.PI / 180;
    const imageWidth = Math.min(this.width, this.height * 0.75), imageHeight = imageWidth / 0.75;
    gl.useProgram(this.program); gl.uniform2f(this.size, imageWidth, imageHeight);
    gl.uniform2f(this.fit, imageWidth / this.width, imageHeight / this.height);
    gl.uniform3f(this.angles, pose.yaw * radians, pose.pitch * radians, pose.roll * radians);
    gl.uniform2f(this.shift, pose.tx, pose.ty);
    gl.bindTexture(gl.TEXTURE_2D, this.texture);
    gl.clearColor(0, 0, 0, 0); gl.clear(gl.COLOR_BUFFER_BIT); gl.drawElements(gl.TRIANGLES, this.count, gl.UNSIGNED_SHORT, 0);
  }
  dispose() {
    if (!this.gl.isContextLost()) {
      if (this.texture) this.gl.deleteTexture(this.texture);
      this.buffers.forEach(buffer => this.gl.deleteBuffer(buffer));
      if (this.program) this.gl.deleteProgram(this.program);
    }
    this.texture = null; this.buffers = []; this.program = null;
  }
}
