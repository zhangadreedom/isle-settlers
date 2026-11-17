// Three.js board renderer.
//
// * WebGPURenderer, falling back to WebGL2 automatically.
// * Render on demand: a frame is drawn only when the camera moves or the
//   game state changes, so an idle board costs no GPU time.
// * Repeated objects (tiles, decorations, pieces) are InstancedMeshes, which
//   keeps the whole scene to a few dozen draw calls.
// * Positions come straight from the engine's lattice tables.

import * as THREE from 'three/webgpu';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { CITY_FLAG, EMPTY, type Topology } from '../engine';
import { BEACH, PLAYER_COLORS, RESOURCE_COLORS, RESOURCE_LABELS, SEA, SKY, TERRAIN_COLORS } from './palette';

export interface BoardData {
  terrain: Uint8Array;
  numbers: Uint8Array;
  harbors: Uint8Array;
}

export interface PieceData {
  buildings: Uint8Array;
  roads: Uint8Array;
  robber: number;
}

const TILE_HEIGHT = 0.22;
const TOP = TILE_HEIGHT;
const SQRT3_2 = Math.sqrt(3) / 2;
const PIPS = [0, 0, 1, 2, 3, 4, 5, 6, 5, 4, 3, 2, 1];
const BASE_CAMERA_DISTANCE = 13.5;
const CAMERA_DIR = new THREE.Vector3(0, 10.5, 8.5).normalize();

const lattice = (x: number, y: number): [number, number] => [x * SQRT3_2, y * 0.5];

/** Small deterministic PRNG so decorations stay put between renders. */
function prng(seed: number) {
  let s = seed >>> 0 || 1;
  return () => {
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    return (s >>> 0) / 4294967296;
  };
}

function std(color: number, extra: Partial<THREE.MeshStandardMaterialParameters> = {}) {
  return new THREE.MeshStandardMaterial({ color, roughness: 0.85, flatShading: true, ...extra });
}

// ---- shared geometry ----
const hexGeometry = new THREE.CylinderGeometry(0.97, 0.97, TILE_HEIGHT, 6);

function prism(width: number, height: number, depth: number) {
  const shape = new THREE.Shape();
  shape.moveTo(-width / 2, 0);
  shape.lineTo(width / 2, 0);
  shape.lineTo(0, height);
  shape.closePath();
  const g = new THREE.ExtrudeGeometry(shape, { depth, bevelEnabled: false });
  g.translate(0, 0, -depth / 2);
  return g;
}

function box(w: number, h: number, d: number, x: number, y: number, z: number) {
  return new THREE.BoxGeometry(w, h, d).translate(x, y, z).toNonIndexed();
}

const settlementGeometry = mergeGeometries([
  box(0.2, 0.15, 0.2, 0, 0.075, 0),
  prism(0.26, 0.13, 0.22).translate(0, 0.15, 0),
])!;

const cityGeometry = mergeGeometries([
  box(0.38, 0.15, 0.22, 0, 0.075, 0),
  box(0.17, 0.3, 0.22, -0.1, 0.15, 0),
  prism(0.24, 0.12, 0.22).translate(0.09, 0.15, 0),
  prism(0.21, 0.1, 0.24).translate(-0.1, 0.3, 0),
])!;

const roadGeometry = new THREE.BoxGeometry(0.62, 0.08, 0.1);
const tokenGeometry = new THREE.CylinderGeometry(0.25, 0.25, 0.05, 24);
const labelGeometry = new THREE.PlaneGeometry(0.5, 0.5).rotateX(-Math.PI / 2);

interface Decoration {
  geometry: THREE.BufferGeometry;
  material: THREE.Material;
  count: number;
  scale: [number, number];
  squash?: number;
  /** Optional cap stacked on each instance (snow on peaks). */
  cap?: { geometry: THREE.BufferGeometry; material: THREE.Material; lift: number };
}

const DECORATIONS: Record<number, Decoration> = {
  0: { geometry: new THREE.DodecahedronGeometry(0.12, 0), material: std(0x9b4a2a), count: 4, scale: [0.8, 1.2], squash: 0.6 },
  1: { geometry: new THREE.ConeGeometry(0.13, 0.42, 6).translate(0, 0.21, 0), material: std(0x1f5e36), count: 6, scale: [0.75, 1.2] },
  2: { geometry: new THREE.IcosahedronGeometry(0.075, 0).translate(0, 0.06, 0), material: std(0xf7f7f2), count: 4, scale: [0.9, 1.15] },
  3: { geometry: new THREE.ConeGeometry(0.09, 0.22, 5).translate(0, 0.11, 0), material: std(0xc99a2e), count: 5, scale: [0.85, 1.15] },
  4: {
    geometry: new THREE.ConeGeometry(0.28, 0.6, 5).translate(0, 0.3, 0),
    material: std(0x6f747d),
    count: 3,
    scale: [0.75, 1.15],
    cap: { geometry: new THREE.ConeGeometry(0.108, 0.23, 5).translate(0, 0.485, 0), material: std(0xf4f6f8), lift: 0.002 },
  },
  5: { geometry: new THREE.SphereGeometry(0.25, 8, 5), material: std(0xd8c08a), count: 2, scale: [0.8, 1.2], squash: 0.3 },
};

function numberTexture(n: number) {
  const size = 128;
  const c = document.createElement('canvas');
  c.width = c.height = size;
  const ctx = c.getContext('2d')!;
  const red = n === 6 || n === 8;
  ctx.fillStyle = red ? '#b8322a' : '#2b2b2b';
  ctx.font = `bold ${red ? 64 : 58}px system-ui, sans-serif`;
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.fillText(String(n), size / 2, size / 2 - 8);
  const pips = PIPS[n];
  for (let i = 0; i < pips; i++) {
    ctx.beginPath();
    ctx.arc(size / 2 + (i - (pips - 1) / 2) * 11, size / 2 + 34, 4, 0, Math.PI * 2);
    ctx.fill();
  }
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  t.anisotropy = 4;
  return t;
}

function harborTexture(code: number) {
  const size = 128;
  const c = document.createElement('canvas');
  c.width = c.height = size;
  const ctx = c.getContext('2d')!;
  ctx.beginPath();
  ctx.arc(size / 2, size / 2, 58, 0, Math.PI * 2);
  ctx.fillStyle = '#f6efdc';
  ctx.fill();
  ctx.lineWidth = 10;
  ctx.strokeStyle = code === 5 ? '#5b6770' : `#${RESOURCE_COLORS[code].toString(16).padStart(6, '0')}`;
  ctx.stroke();
  ctx.fillStyle = '#2b2b2b';
  ctx.textAlign = 'center';
  ctx.textBaseline = 'middle';
  ctx.font = 'bold 40px system-ui, sans-serif';
  ctx.fillText(code === 5 ? '3:1' : '2:1', size / 2, code === 5 ? size / 2 : size / 2 - 14);
  if (code !== 5) {
    ctx.font = 'bold 30px system-ui, sans-serif';
    ctx.fillText(RESOURCE_LABELS[code], size / 2, size / 2 + 24);
  }
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  return t;
}

type Backend = 'WebGPU' | 'WebGL2';

/** Minimal slice of the WebGPU device API we rely on (no @webgpu/types needed). */
interface DeviceLike {
  lost: Promise<{ reason: string; message: string }>;
}

function createRenderer(forceWebGL: boolean) {
  return new THREE.WebGPURenderer({ antialias: true, forceWebGL });
}

export class BoardRenderer {
  /** Called when the renderer switches backend (WebGPU device loss). */
  onBackendChange?: (backend: Backend) => void;
  private _backend: Backend = 'WebGL2';
  private recovering = false;
  private readonly scene = new THREE.Scene();
  private readonly camera = new THREE.PerspectiveCamera(38, 1, 0.1, 120);
  private readonly controls: OrbitControls;
  private readonly board = new THREE.Group();
  private readonly roads: THREE.InstancedMesh;
  private readonly settlements: THREE.InstancedMesh;
  private readonly cities: THREE.InstancedMesh;
  private readonly robber = new THREE.Group();
  private readonly resizeObserver: ResizeObserver;
  private readonly tileXZ: [number, number][];
  private readonly vertexXZ: [number, number][];
  private readonly numberTextures = new Map<number, THREE.Texture>();
  private renderQueued = false;
  private disposables: { dispose(): void }[] = [];

  /** `forceWebGL` skips WebGPU (also available as `?renderer=webgl`). */
  static async create(container: HTMLElement, topology: Topology, forceWebGL = false) {
    const renderer = createRenderer(forceWebGL);
    await renderer.init();
    return new BoardRenderer(container, topology, renderer);
  }

  private constructor(
    private readonly container: HTMLElement,
    private readonly topology: Topology,
    private renderer: THREE.WebGPURenderer,
  ) {
    this.attachRenderer(renderer);

    this.tileXZ = [];
    for (let i = 0; i < topology.tilePos.length; i += 2) this.tileXZ.push(lattice(topology.tilePos[i], topology.tilePos[i + 1]));
    this.vertexXZ = [];
    for (let i = 0; i < topology.vertexPos.length; i += 2) this.vertexXZ.push(lattice(topology.vertexPos[i], topology.vertexPos[i + 1]));

    this.scene.background = new THREE.Color(SKY);
    this.scene.fog = new THREE.Fog(SKY, 22, 60);

    // Bound to the container (not the canvas) so a renderer swap keeps input working.
    this.controls = new OrbitControls(this.camera, container);
    this.controls.enableDamping = true;
    this.controls.enablePan = false;
    this.controls.minPolarAngle = 0.15;
    this.controls.maxPolarAngle = 1.2;
    this.controls.addEventListener('change', () => this.requestRender());

    this.scene.add(new THREE.HemisphereLight(0xdff3ff, 0x2d4a3a, 1.3));
    const sun = new THREE.DirectionalLight(0xfff1d6, 2.4);
    sun.position.set(-6, 12, 5);
    sun.castShadow = true;
    sun.shadow.mapSize.set(2048, 2048);
    Object.assign(sun.shadow.camera, { left: -7, right: 7, top: 7, bottom: -7, near: 1, far: 30 });
    sun.shadow.bias = -0.0005;
    this.scene.add(sun);

    const sea = new THREE.Mesh(new THREE.PlaneGeometry(120, 120).rotateX(-Math.PI / 2), std(SEA, { roughness: 0.55, flatShading: false }));
    sea.position.y = -0.14;
    sea.receiveShadow = true;
    const beach = new THREE.Mesh(new THREE.CylinderGeometry(5.25, 5.7, 0.24, 6).rotateY(Math.PI / 6), std(BEACH));
    beach.position.y = -0.12;
    beach.receiveShadow = true;
    this.scene.add(sea, beach, this.board);

    const pieceMaterial = std(0xffffff, { roughness: 0.6 });
    this.roads = this.instanced(roadGeometry, pieceMaterial, topology.edgeVertices.length / 2, true);
    this.settlements = this.instanced(settlementGeometry, pieceMaterial, this.vertexXZ.length, true);
    this.cities = this.instanced(cityGeometry, pieceMaterial, this.vertexXZ.length, true);
    this.scene.add(this.roads, this.settlements, this.cities);

    const robberMaterial = std(0x2d2d33, { roughness: 0.5 });
    const body = new THREE.Mesh(new THREE.CylinderGeometry(0.08, 0.13, 0.3, 10).translate(0, 0.15, 0), robberMaterial);
    const head = new THREE.Mesh(new THREE.SphereGeometry(0.085, 12, 8).translate(0, 0.36, 0), robberMaterial);
    body.castShadow = head.castShadow = true;
    this.robber.add(body, head);
    this.scene.add(this.robber);

    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(container);
    this.resize();
  }

  get backend(): Backend {
    return this._backend;
  }

  /** Draw calls in the last frame (for the HUD). */
  get drawCalls() {
    return this.renderer.info.render.drawCalls;
  }

  /** Rebuilds tiles, number tokens, decorations and harbors for a new map. */
  setBoard({ terrain, numbers, harbors }: BoardData) {
    for (const d of this.disposables) d.dispose();
    this.disposables = [];
    this.board.clear();

    const tileMaterial = std(0xffffff);
    const tiles = this.instanced(hexGeometry, tileMaterial, terrain.length, true);
    tiles.castShadow = false;
    const m = new THREE.Matrix4();
    const color = new THREE.Color();
    terrain.forEach((t, i) => {
      const [x, z] = this.tileXZ[i];
      tiles.setMatrixAt(i, m.makeTranslation(x, TILE_HEIGHT / 2, z));
      tiles.setColorAt(i, color.setHex(TERRAIN_COLORS[t]));
    });
    tiles.count = terrain.length;
    this.board.add(tiles);
    this.disposables.push(tileMaterial, tiles);

    this.addDecorations(terrain);
    this.addTokens(numbers);
    this.addHarbors(harbors);
    this.requestRender();
  }

  /** Updates roads, buildings and the robber from engine arrays. */
  setPieces({ buildings, roads, robber }: PieceData) {
    const m = new THREE.Matrix4();
    const q = new THREE.Quaternion();
    const s = new THREE.Vector3(1, 1, 1);
    const p = new THREE.Vector3();
    const up = new THREE.Vector3(0, 1, 0);
    const color = new THREE.Color();

    let n = 0;
    const ev = this.topology.edgeVertices;
    roads.forEach((owner, e) => {
      if (owner === EMPTY) return;
      const [ax, az] = this.vertexXZ[ev[2 * e]];
      const [bx, bz] = this.vertexXZ[ev[2 * e + 1]];
      q.setFromAxisAngle(up, -Math.atan2(bz - az, bx - ax));
      this.roads.setMatrixAt(n, m.compose(p.set((ax + bx) / 2, TOP + 0.04, (az + bz) / 2), q, s));
      this.roads.setColorAt(n++, color.setHex(PLAYER_COLORS[owner]));
    });
    this.commit(this.roads, n);

    let ns = 0;
    let nc = 0;
    buildings.forEach((b, v) => {
      if (b === EMPTY) return;
      const owner = b & ~CITY_FLAG;
      const [x, z] = this.vertexXZ[v];
      // Face buildings towards the island centre for a little variety.
      q.setFromAxisAngle(up, -Math.atan2(z, x));
      m.compose(p.set(x, TOP, z), q, s);
      color.setHex(PLAYER_COLORS[owner]);
      if (b & CITY_FLAG) {
        this.cities.setMatrixAt(nc, m);
        this.cities.setColorAt(nc++, color);
      } else {
        this.settlements.setMatrixAt(ns, m);
        this.settlements.setColorAt(ns++, color);
      }
    });
    this.commit(this.settlements, ns);
    this.commit(this.cities, nc);

    const [rx, rz] = this.tileXZ[robber];
    this.robber.position.set(rx + 0.42, TOP, rz + 0.12);
    this.requestRender();
  }

  requestRender() {
    if (this.renderQueued) return;
    this.renderQueued = true;
    requestAnimationFrame(() => {
      this.renderQueued = false;
      this.controls.update(); // emits 'change' while damping, which queues the next frame
      this.renderer.render(this.scene, this.camera);
    });
  }

  dispose() {
    this.resizeObserver.disconnect();
    this.controls.dispose();
    for (const d of this.disposables) d.dispose();
    for (const t of this.numberTextures.values()) t.dispose();
    this.renderer.dispose();
    this.renderer.domElement.remove();
  }

  // ---- internals ----

  private attachRenderer(renderer: THREE.WebGPURenderer) {
    const backend = renderer.backend as { isWebGPUBackend?: boolean; device?: DeviceLike };
    this._backend = backend.isWebGPUBackend ? 'WebGPU' : 'WebGL2';
    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    renderer.shadowMap.enabled = true;
    renderer.shadowMap.type = THREE.PCFShadowMap;
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    this.container.appendChild(renderer.domElement);
    // A WebGPU device can be lost at any time (driver reset, GPU process
    // crash, broken adapters). Rebuild on WebGL2 instead of showing nothing.
    backend.device?.lost.then((info) => {
      if (info.reason !== 'destroyed' && renderer === this.renderer) void this.fallBackToWebGL(info.message);
    });
  }

  private async fallBackToWebGL(reason: string) {
    if (this.recovering) return;
    this.recovering = true;
    console.warn(`WebGPU device lost (${reason}); switching to WebGL2.`);
    const old = this.renderer;
    const next = createRenderer(true);
    await next.init();
    old.domElement.remove();
    try {
      old.dispose();
    } catch {
      // The lost device may refuse to release resources; nothing to do.
    }
    this.renderer = next;
    this.attachRenderer(next);
    this.recovering = false;
    this.onBackendChange?.(this._backend);
    this.resize();
  }

  /**
   * `colored` allocates per-instance colors up front: the WebGPU renderer
   * builds the material program on first use, so the attribute has to exist
   * before the first frame for colors set later to show up.
   */
  private instanced(geometry: THREE.BufferGeometry, material: THREE.Material, capacity: number, colored = false) {
    const mesh = new THREE.InstancedMesh(geometry, material, capacity);
    if (colored) mesh.instanceColor = new THREE.InstancedBufferAttribute(new Float32Array(capacity * 3).fill(1), 3);
    mesh.count = 0;
    mesh.castShadow = true;
    mesh.receiveShadow = true;
    mesh.frustumCulled = false;
    return mesh;
  }

  private commit(mesh: THREE.InstancedMesh, count: number) {
    mesh.count = count;
    mesh.instanceMatrix.needsUpdate = true;
    if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
  }

  private addDecorations(terrain: Uint8Array) {
    const m = new THREE.Matrix4();
    const q = new THREE.Quaternion();
    const s = new THREE.Vector3();
    const p = new THREE.Vector3();
    const up = new THREE.Vector3(0, 1, 0);

    for (const [code, deco] of Object.entries(DECORATIONS)) {
      const tilesOfType = [...terrain.keys()].filter((i) => terrain[i] === Number(code));
      const capacity = tilesOfType.length * deco.count;
      if (capacity === 0) continue;
      const mesh = this.instanced(deco.geometry, deco.material, capacity);
      const cap = deco.cap && this.instanced(deco.cap.geometry, deco.cap.material, capacity);
      let n = 0;
      for (const t of tilesOfType) {
        const rand = prng(t * 7919 + Number(code) * 104729);
        const [cx, cz] = this.tileXZ[t];
        const offset = rand() * Math.PI * 2;
        for (let k = 0; k < deco.count; k++) {
          const angle = offset + (k / deco.count) * Math.PI * 2 + (rand() - 0.5) * 0.6;
          const r = 0.42 + rand() * 0.22;
          const sc = deco.scale[0] + rand() * (deco.scale[1] - deco.scale[0]);
          q.setFromAxisAngle(up, rand() * Math.PI * 2);
          s.set(sc, sc * (deco.squash ?? 1), sc);
          m.compose(p.set(cx + Math.cos(angle) * r, TOP, cz + Math.sin(angle) * r), q, s);
          mesh.setMatrixAt(n, m);
          cap?.setMatrixAt(n, m);
          n++;
        }
      }
      this.commit(mesh, n);
      this.board.add(mesh);
      this.disposables.push(mesh);
      if (cap) {
        this.commit(cap, n);
        this.board.add(cap);
        this.disposables.push(cap);
      }
    }
  }

  private addTokens(numbers: Uint8Array) {
    const discMaterial = std(0xf3ead2, { roughness: 0.7 });
    this.disposables.push(discMaterial);
    numbers.forEach((n, i) => {
      if (n === 0) return;
      const [x, z] = this.tileXZ[i];
      const disc = new THREE.Mesh(tokenGeometry, discMaterial);
      disc.position.set(x, TOP + 0.025, z);
      disc.castShadow = true;
      let tex = this.numberTextures.get(n);
      if (!tex) this.numberTextures.set(n, (tex = numberTexture(n)));
      const labelMaterial = new THREE.MeshStandardMaterial({ map: tex, transparent: true, roughness: 0.7 });
      const label = new THREE.Mesh(labelGeometry, labelMaterial);
      label.position.set(x, TOP + 0.052, z);
      this.board.add(disc, label);
      this.disposables.push(labelMaterial);
    });
  }

  private addHarbors(harbors: Uint8Array) {
    const plankMaterial = std(0x8a5a35);
    const planks = this.instanced(new THREE.BoxGeometry(1, 0.05, 0.09), plankMaterial, harbors.length * 2);
    const docks = this.instanced(new THREE.BoxGeometry(0.34, 0.08, 0.34), plankMaterial, harbors.length);
    this.disposables.push(plankMaterial, planks, docks, planks.geometry, docks.geometry);
    const m = new THREE.Matrix4();
    const q = new THREE.Quaternion();
    const s = new THREE.Vector3();
    const p = new THREE.Vector3();
    const up = new THREE.Vector3(0, 1, 0);
    const ev = this.topology.edgeVertices;

    harbors.forEach((code, slot) => {
      const e = this.topology.harborEdges[slot];
      const a = this.vertexXZ[ev[2 * e]];
      const b = this.vertexXZ[ev[2 * e + 1]];
      const mx = (a[0] + b[0]) / 2;
      const mz = (a[1] + b[1]) / 2;
      const len = Math.hypot(mx, mz);
      const dock: [number, number] = [mx + (mx / len) * 0.5, mz + (mz / len) * 0.5];
      docks.setMatrixAt(slot, m.makeTranslation(dock[0], -0.02, dock[1]));
      [a, b].forEach((v, k) => {
        const dx = dock[0] - v[0];
        const dz = dock[1] - v[1];
        q.setFromAxisAngle(up, -Math.atan2(dz, dx));
        s.set(Math.hypot(dx, dz), 1, 1);
        planks.setMatrixAt(slot * 2 + k, m.compose(p.set((v[0] + dock[0]) / 2, 0.02, (v[1] + dock[1]) / 2), q, s));
      });
      const tex = harborTexture(code);
      const mat = new THREE.SpriteMaterial({ map: tex });
      const label = new THREE.Sprite(mat);
      label.scale.set(0.55, 0.55, 1);
      label.position.set(dock[0], 0.45, dock[1]);
      this.board.add(label);
      this.disposables.push(tex, mat);
    });
    this.commit(planks, harbors.length * 2);
    this.commit(docks, harbors.length);
    this.board.add(planks, docks);
  }

  private resize() {
    const w = Math.max(1, this.container.clientWidth);
    const h = Math.max(1, this.container.clientHeight);
    this.renderer.setSize(w, h);
    this.camera.aspect = w / h;
    // Keep the whole island in view on narrow (portrait) screens.
    const halfV = THREE.MathUtils.degToRad(this.camera.fov / 2);
    const halfH = Math.atan(Math.tan(halfV) * this.camera.aspect);
    const distance = Math.max(BASE_CAMERA_DISTANCE, 5.8 / Math.tan(halfH));
    this.camera.position.copy(CAMERA_DIR).multiplyScalar(distance);
    this.controls.minDistance = distance * 0.45;
    this.controls.maxDistance = distance * 1.6;
    this.camera.updateProjectionMatrix();
    this.controls.update();
    this.requestRender();
  }
}
