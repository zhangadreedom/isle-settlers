// Three.js board renderer.
//
// * WebGPURenderer, falling back to WebGL2 automatically (and rebuilding on
//   WebGL2 if a WebGPU device is lost).
// * Render on demand: frames are drawn only while the camera moves, an
//   animation runs or the state changes; an idle board costs no GPU time.
// * Repeated objects (tiles, decorations, pieces, markers) are
//   InstancedMeshes, keeping the scene to a few dozen draw calls.
// * Picking intersects the pointer ray with the board plane and snaps to the
//   nearest highlighted spot; no mesh raycasting.

import * as THREE from 'three/webgpu';
import { OrbitControls } from 'three/addons/controls/OrbitControls.js';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
import { CITY_FLAG, EMPTY, type Topology } from '../engine';
import { BEACH, PLAYER_COLORS, RESOURCE_COLORS, RESOURCE_LABELS, SEA, SKY, TERRAIN_COLORS } from './palette';

/** Static layout as numeric codes (see palette for terrain/harbor codes). */
export interface BoardData {
  terrain: number[];
  numbers: number[];
  harbors: number[];
  /** Island per tile: 0 for the sea, 1.. for land. */
  islands: number[];
}

export interface PieceData {
  buildings: number[];
  roads: number[];
  /** Seafaring: ship owner per edge (or EMPTY). */
  ships?: number[];
  robber: number;
  /** Seafaring: the pirate's sea tile. */
  pirate?: number | null;
}

/** Terrain code of the sea (see palette). */
const SEA_TERRAIN = 6;
/** Height of ships and the pirate above the water plane. */
const WATER = -0.06;

export type PickKind = 'vertex' | 'edge' | 'tile';

/** Screen space (CSS px) covered by HUD panels on each side. */
export interface Insets {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

export interface Highlights {
  kind: PickKind;
  ids: number[];
}

type Backend = 'WebGPU' | 'WebGL2';

/** Minimal slice of the WebGPU device API we rely on (no @webgpu/types needed). */
interface DeviceLike {
  lost: Promise<{ reason: string; message: string }>;
}

const TILE_HEIGHT = 0.22;
const TOP = TILE_HEIGHT;
const SQRT3_2 = Math.sqrt(3) / 2;
const PIPS = [0, 0, 1, 2, 3, 4, 5, 6, 5, 4, 3, 2, 1];
/** Margin (world units) kept visible around the outermost tile centres: tiles plus harbor labels. */
const MARGIN_X = 1.74;
const MARGIN_Z = 1.42;
/** Instance capacity for per-spot meshes; covers the largest board. */
const MAX_SPOTS = 128;
/** Beach outline: distance from the outermost tile centres at the top and the foot of the sand. */
const BEACH_TOP = 1.55;
const BEACH_FOOT = 1.94;
/** Seafaring islands get a narrower beach so straits stay open water. */
const ISLAND_BEACH_TOP = 1.2;
const ISLAND_BEACH_FOOT = 1.44;
const CAMERA_DIR = new THREE.Vector3(0, 10.5, 8.5).normalize();
const DROP_MS = 420;
const ROBBER_MS = 550;
/** Pick radius (world units) per target kind. */
const PICK_RADIUS: Record<PickKind, number> = { vertex: 0.38, edge: 0.32, tile: 0.9 };
const ACCENT = 0xf0b429;

const lattice = (x: number, y: number): [number, number] => [x * SQRT3_2, y * 0.5];
const easeOutBack = (t: number) => 1 + 2.2 * (t - 1) ** 3 + 1.2 * (t - 1) ** 2;
const easeInOut = (t: number) => (t < 0.5 ? 2 * t * t : 1 - (-2 * t + 2) ** 2 / 2);

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

/** Convex hull (counter-clockwise, no collinear points) of 2D points. */
function convexHull(points: [number, number][]): [number, number][] {
  const pts = [...points].sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const cross = (o: [number, number], a: [number, number], b: [number, number]) =>
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
  const half = (list: [number, number][]) => {
    const out: [number, number][] = [];
    for (const p of list) {
      while (out.length >= 2 && cross(out[out.length - 2], out[out.length - 1], p) <= 1e-9) out.pop();
      out.push(p);
    }
    out.pop();
    return out;
  };
  return [...half(pts), ...half([...pts].reverse())];
}

/** Moves every edge of a convex counter-clockwise polygon outwards by `d`. */
function offsetPolygon(poly: [number, number][], d: number): [number, number][] {
  const n = poly.length;
  const normal = (i: number): [number, number] => {
    const [ax, az] = poly[i];
    const [bx, bz] = poly[(i + 1) % n];
    const len = Math.hypot(bx - ax, bz - az);
    return [(bz - az) / len, -(bx - ax) / len];
  };
  // Corners sharper than a right angle are bevelled (two points), so a
  // small island's beach does not shoot out a long spike. The choice depends
  // only on the angle, so offsets of one polygon keep the same vertex count.
  return poly.flatMap((p, i): [number, number][] => {
    const n1 = normal((i - 1 + n) % n);
    const n2 = normal(i);
    const dot = n1[0] * n2[0] + n1[1] * n2[1];
    if (dot < 0) {
      return [
        [p[0] + n1[0] * d, p[1] + n1[1] * d],
        [p[0] + n2[0] * d, p[1] + n2[1] * d],
      ];
    }
    const k = d / (1 + dot);
    return [[p[0] + (n1[0] + n2[0]) * k, p[1] + (n1[1] + n2[1]) * k]];
  });
}

/** A sand bank: flat top polygon `top` at y = 0 sloping down to `foot` at y = -height. */
function bankGeometry(top: [number, number][], foot: [number, number][], height: number) {
  const pos: number[] = [];
  const n = top.length;
  // Hull points are counter-clockwise in (x, z); seen from above (+y) that is clockwise.
  for (let i = 1; i < n - 1; i++) pos.push(top[0][0], 0, top[0][1], top[i + 1][0], 0, top[i + 1][1], top[i][0], 0, top[i][1]);
  for (let i = 0; i < n; i++) {
    const j = (i + 1) % n;
    const [a, b, c, d] = [top[i], top[j], foot[i], foot[j]];
    pos.push(a[0], 0, a[1], b[0], 0, b[1], c[0], -height, c[1]);
    pos.push(b[0], 0, b[1], d[0], -height, d[1], c[0], -height, c[1]);
  }
  const g = new THREE.BufferGeometry();
  g.setAttribute('position', new THREE.Float32BufferAttribute(pos, 3));
  g.computeVertexNormals();
  return g;
}

function std(color: number, extra: Partial<THREE.MeshStandardMaterialParameters> = {}) {
  return new THREE.MeshStandardMaterial({ color, roughness: 0.85, flatShading: true, ...extra });
}

function createRenderer(forceWebGL: boolean) {
  return new THREE.WebGPURenderer({ antialias: true, forceWebGL });
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
  box(0.24, 0.18, 0.24, 0, 0.09, 0),
  prism(0.31, 0.16, 0.26).translate(0, 0.18, 0),
])!;

const cityGeometry = mergeGeometries([
  box(0.44, 0.18, 0.26, 0, 0.09, 0),
  box(0.2, 0.36, 0.26, -0.12, 0.18, 0),
  prism(0.28, 0.14, 0.26).translate(0.11, 0.18, 0),
  prism(0.25, 0.12, 0.28).translate(-0.12, 0.36, 0),
])!;

const roadGeometry = new THREE.BoxGeometry(0.64, 0.09, 0.12);
/** Flat triangle pointing +x (a bow), `length` long, `width` wide, `thickness` high. */
function bow(width: number, length: number, thickness: number) {
  return prism(width, length, thickness).rotateX(Math.PI / 2).rotateY(Math.PI / 2);
}

const mast = (r: number, h: number, x: number, y: number) => new THREE.CylinderGeometry(r, r, h, 6).translate(x, y, 0).toNonIndexed();

/** A small boat: hull, bow, cabin, mast and sail; bow along +x. */
const shipGeometry = mergeGeometries([
  box(0.4, 0.09, 0.17, 0, 0.045, 0),
  bow(0.17, 0.12, 0.09).translate(0.2, 0.045, 0),
  box(0.12, 0.07, 0.11, -0.11, 0.125, 0),
  mast(0.012, 0.36, 0.04, 0.27),
  box(0.02, 0.22, 0.2, 0.06, 0.3, 0),
])!;
/** The pirate: a bigger, dark ship with two sails. */
const pirateHull = mergeGeometries([
  box(0.56, 0.12, 0.24, 0, 0.06, 0),
  bow(0.24, 0.16, 0.12).translate(0.28, 0.06, 0),
  box(0.16, 0.1, 0.2, -0.2, 0.17, 0),
  mast(0.015, 0.5, 0.08, 0.37),
  mast(0.013, 0.36, -0.1, 0.3),
])!;
const pirateSails = mergeGeometries([box(0.025, 0.3, 0.28, 0.1, 0.4, 0), box(0.022, 0.2, 0.2, -0.08, 0.32, 0)])!;
const seaRingGeometry = new THREE.RingGeometry(0.9, 0.97, 6, 1, Math.PI / 6).rotateX(-Math.PI / 2);
const tokenGeometry = new THREE.CylinderGeometry(0.25, 0.25, 0.05, 24);
const labelGeometry = new THREE.PlaneGeometry(0.5, 0.5).rotateX(-Math.PI / 2);
const vertexMarkerGeometry = new THREE.CylinderGeometry(0.14, 0.14, 0.05, 20);
const edgeMarkerGeometry = new THREE.BoxGeometry(0.5, 0.05, 0.14);
const tileMarkerGeometry = new THREE.RingGeometry(0.72, 0.86, 6, 1, Math.PI / 6).rotateX(-Math.PI / 2);

interface Decoration {
  geometry: THREE.BufferGeometry;
  material: THREE.Material;
  count: number;
  scale: [number, number];
  squash?: number;
  /** Optional cap stacked on each instance (snow on peaks). */
  cap?: { geometry: THREE.BufferGeometry; material: THREE.Material };
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
    cap: { geometry: new THREE.ConeGeometry(0.108, 0.23, 5).translate(0, 0.485, 0), material: std(0xf4f6f8) },
  },
  5: { geometry: new THREE.SphereGeometry(0.25, 8, 5), material: std(0xd8c08a), count: 2, scale: [0.8, 1.2], squash: 0.3 },
  7: {
    geometry: new THREE.OctahedronGeometry(0.1, 0).translate(0, 0.06, 0),
    material: std(0xffcf3a, { metalness: 0.55, roughness: 0.25, emissive: 0x6b4a00 }),
    count: 7,
    scale: [0.8, 1.4],
    squash: 0.75,
  },
};

function canvasTexture(draw: (ctx: CanvasRenderingContext2D, size: number) => void) {
  const size = 128;
  const c = document.createElement('canvas');
  c.width = c.height = size;
  draw(c.getContext('2d')!, size);
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  t.anisotropy = 4;
  return t;
}

function numberTexture(n: number) {
  return canvasTexture((ctx, size) => {
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
  });
}

function harborTexture(code: number) {
  return canvasTexture((ctx, size) => {
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
  });
}

/** One placed piece; `born` drives the drop-in animation. */
interface Piece {
  x: number;
  z: number;
  angle: number;
  owner: number;
  born: number;
}

export class BoardRenderer {
  /** Called when the renderer switches backend (WebGPU device loss). */
  onBackendChange?: (backend: Backend) => void;
  /** Called when a highlighted spot is clicked or tapped. */
  onPick?: (kind: PickKind, id: number) => void;

  private _backend: Backend = 'WebGL2';
  private recovering = false;
  private readonly scene = new THREE.Scene();
  private readonly camera = new THREE.PerspectiveCamera(38, 1, 0.1, 120);
  private readonly controls: OrbitControls;
  private readonly board = new THREE.Group();
  private readonly roads: THREE.InstancedMesh;
  private readonly ships: THREE.InstancedMesh;
  private readonly pirate = new THREE.Group();
  private readonly settlements: THREE.InstancedMesh;
  private readonly cities: THREE.InstancedMesh;
  private readonly markers: Record<PickKind, THREE.InstancedMesh>;
  private readonly robber = new THREE.Group();
  private readonly resizeObserver: ResizeObserver;
  private readonly beach: THREE.Mesh;
  private readonly sun: THREE.DirectionalLight;
  private topology: Topology | null = null;
  private tileXZ: [number, number][] = [];
  private vertexXZ: [number, number][] = [];
  private edgeXZ: { x: number; z: number; angle: number }[] = [];
  /** Unit vector pointing out to sea from each harbor slot. */
  private harborOut: [number, number][] = [];
  /** Tiles on either side of each edge. */
  private edgeTiles: number[][] = [];
  /** Per edge: unit direction towards open water (zero between two sea tiles). */
  private edgeSeaward: [number, number][] = [];
  /** Half extents (world units) of the area that must stay in view. */
  private extent = { x: 5.2, z: 4.42 };
  private readonly numberTextures = new Map<number, THREE.Texture>();
  private readonly raycaster = new THREE.Raycaster();
  private readonly plane = new THREE.Plane(new THREE.Vector3(0, 1, 0), -TOP);
  private renderQueued = false;
  private disposables: { dispose(): void }[] = [];

  // Pieces keyed by spot so new ones can be animated.
  private roadPieces = new Map<number, Piece>();
  private shipPieces = new Map<number, Piece>();
  private pirateFrom: [number, number] | null = null;
  private pirateTo: [number, number] | null = null;
  private pirateStart = 0;
  private buildingPieces = new Map<number, Piece & { city: boolean }>();
  private robberFrom: [number, number] | null = null;
  private robberTo: [number, number] = [0, 0];
  private robberStart = 0;

  private insets: Insets = { top: 0, right: 0, bottom: 0, left: 0 };
  /** Camera distance that frames the island; user zoom is kept relative to it. */
  private fitDistance = 0;
  private highlights: Highlights | null = null;
  private hovered = -1;
  private pointerDown: { x: number; y: number; t: number } | null = null;

  /** `forceWebGL` skips WebGPU (also available as `?renderer=webgl`). */
  static async create(container: HTMLElement, forceWebGL = false) {
    const renderer = createRenderer(forceWebGL);
    await renderer.init();
    return new BoardRenderer(container, renderer);
  }

  private constructor(
    private readonly container: HTMLElement,
    private renderer: THREE.WebGPURenderer,
  ) {
    this.attachRenderer(renderer);

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
    this.sun = sun;

    const sea = new THREE.Mesh(new THREE.PlaneGeometry(120, 120).rotateX(-Math.PI / 2), std(SEA, { roughness: 0.55, flatShading: false }));
    sea.position.y = -0.14;
    sea.receiveShadow = true;
    this.beach = new THREE.Mesh(new THREE.BufferGeometry(), std(BEACH));
    this.beach.position.y = 0;
    this.beach.receiveShadow = true;
    this.scene.add(sea, this.beach, this.board);

    const pieceMaterial = std(0xffffff, { roughness: 0.6 });
    this.roads = this.instanced(roadGeometry, pieceMaterial, MAX_SPOTS, true);
    this.ships = this.instanced(shipGeometry, pieceMaterial, MAX_SPOTS, true);
    this.scene.add(this.ships);
    this.settlements = this.instanced(settlementGeometry, pieceMaterial, MAX_SPOTS, true);
    this.cities = this.instanced(cityGeometry, pieceMaterial, MAX_SPOTS, true);
    this.scene.add(this.roads, this.settlements, this.cities);

    const markerMaterial = new THREE.MeshBasicMaterial({ color: 0xffffff, transparent: true, opacity: 0.85, depthWrite: false });
    this.markers = {
      vertex: this.instanced(vertexMarkerGeometry, markerMaterial, MAX_SPOTS, true),
      edge: this.instanced(edgeMarkerGeometry, markerMaterial, MAX_SPOTS, true),
      tile: this.instanced(tileMarkerGeometry, markerMaterial, MAX_SPOTS, true),
    };
    for (const m of Object.values(this.markers)) {
      m.castShadow = m.receiveShadow = false;
      m.renderOrder = 10;
      this.scene.add(m);
    }

    const robberMaterial = std(0x2d2d33, { roughness: 0.5 });
    const body = new THREE.Mesh(new THREE.CylinderGeometry(0.08, 0.13, 0.3, 10).translate(0, 0.15, 0), robberMaterial);
    const head = new THREE.Mesh(new THREE.SphereGeometry(0.085, 12, 8).translate(0, 0.36, 0), robberMaterial);
    body.castShadow = head.castShadow = true;
    this.robber.add(body, head);
    this.scene.add(this.robber);

    const hull = new THREE.Mesh(pirateHull, std(0x24242b, { roughness: 0.6 }));
    const sails = new THREE.Mesh(pirateSails, std(0x3b3b44, { roughness: 0.8 }));
    hull.castShadow = sails.castShadow = true;
    this.pirate.add(hull, sails);
    this.pirate.visible = false;
    this.scene.add(this.pirate);

    container.addEventListener('pointermove', this.onPointerMove);
    container.addEventListener('pointerdown', this.onPointerDown);
    container.addEventListener('pointerup', this.onPointerUp);
    container.addEventListener('pointerleave', this.onPointerLeave);

    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(container);
    this.resize();
  }

  get backend(): Backend {
    return this._backend;
  }

  /** Island geometry: spot positions, beach, framing and shadow bounds. */
  private setTopology(topology: Topology) {
    this.topology = topology;
    this.tileXZ = [];
    for (let i = 0; i < topology.tilePos.length; i += 2) this.tileXZ.push(lattice(topology.tilePos[i], topology.tilePos[i + 1]));
    this.vertexXZ = [];
    for (let i = 0; i < topology.vertexPos.length; i += 2) this.vertexXZ.push(lattice(topology.vertexPos[i], topology.vertexPos[i + 1]));
    this.edgeXZ = [];
    for (let e = 0; e < topology.edgeVertices.length / 2; e++) {
      const [ax, az] = this.vertexXZ[topology.edgeVertices[2 * e]];
      const [bx, bz] = this.vertexXZ[topology.edgeVertices[2 * e + 1]];
      this.edgeXZ.push({ x: (ax + bx) / 2, z: (az + bz) / 2, angle: -Math.atan2(bz - az, bx - ax) });
    }
    this.edgeTiles = this.edgeXZ.map(() => []);
    for (let t = 0; t < this.tileXZ.length; t++) {
      const corners = topology.tileVertices.subarray(t * 6, t * 6 + 6);
      for (let e = 0; e < this.edgeXZ.length; e++) {
        if (corners.includes(topology.edgeVertices[2 * e]) && corners.includes(topology.edgeVertices[2 * e + 1])) this.edgeTiles[e].push(t);
      }
    }
    const maxX = Math.max(...this.tileXZ.map(([x]) => Math.abs(x)));
    const maxZ = Math.max(...this.tileXZ.map(([, z]) => Math.abs(z)));
    this.extent = { x: maxX + MARGIN_X, z: maxZ + MARGIN_Z };
    const r = Math.max(maxX, maxZ) + 3.5;
    Object.assign(this.sun.shadow.camera, { left: -r, right: r, top: r, bottom: -r });
    this.sun.shadow.camera.updateProjectionMatrix();
    this.resize();
  }

  /**
   * Land-dependent geometry: one beach per island (the classic island is
   * one), which way each harbor and each coastal ship faces.
   */
  private setLand(terrain: number[], islands: number[]) {
    const isSea = (t: number | undefined) => t === undefined || terrain[t] === SEA_TERRAIN;
    const seafaring = terrain.some((t) => t === SEA_TERRAIN);
    const [top, foot] = seafaring ? [ISLAND_BEACH_TOP, ISLAND_BEACH_FOOT] : [BEACH_TOP, BEACH_FOOT];
    const ids = [...new Set(islands.filter((i) => i > 0))];
    const banks = ids.map((id) => {
      const hull = convexHull(this.tileXZ.filter((_, t) => islands[t] === id));
      return bankGeometry(offsetPolygon(hull, top), offsetPolygon(hull, foot), 0.24);
    });
    this.beach.geometry.dispose();
    this.beach.geometry = banks.length > 1 ? mergeGeometries(banks) : (banks[0] ?? new THREE.BufferGeometry());

    // Away from the land tile of an edge, towards the water.
    const outward = (e: number): [number, number] => {
      const landTile = this.edgeTiles[e].find((t) => !isSea(t));
      const seaTile = this.edgeTiles[e].find((t) => isSea(t));
      const { x, z } = this.edgeXZ[e];
      if (landTile === undefined) return [0, 0];
      const [cx, cz] = this.tileXZ[landTile];
      if (seaTile === undefined && this.edgeTiles[e].length > 1) return [0, 0];
      const len = Math.hypot(x - cx, z - cz);
      return [(x - cx) / len, (z - cz) / len];
    };
    this.edgeSeaward = this.edgeXZ.map((_, e) => outward(e));
    this.harborOut = [...this.topology!.harborEdges].map((e) => this.edgeSeaward[e]);
  }

  /** Draw calls in the last frame (for the HUD). */
  get drawCalls() {
    return this.renderer.info.render.drawCalls;
  }

  /** Rebuilds tiles, number tokens, decorations and harbors for a new map. */
  setBoard({ terrain, numbers, harbors, islands }: BoardData, topology: Topology) {
    if (topology !== this.topology) this.setTopology(topology);
    this.setLand(terrain, islands);
    this.camera.position.copy(CAMERA_DIR).multiplyScalar(this.fitDistance || 14);
    this.controls.update();
    for (const d of this.disposables) d.dispose();
    this.disposables = [];
    this.board.clear();
    this.roadPieces.clear();
    this.shipPieces.clear();
    this.buildingPieces.clear();
    this.robberFrom = null;
    this.pirateFrom = this.pirateTo = null;

    // Land tiles are raised hexes; sea tiles inside the map are open water,
    // marked by a faint ring so the sailing grid reads.
    const land = [...terrain.keys()].filter((i) => terrain[i] !== SEA_TERRAIN);
    const sea = [...terrain.keys()].filter((i) => terrain[i] === SEA_TERRAIN);
    const tileMaterial = std(0xffffff);
    const tiles = this.instanced(hexGeometry, tileMaterial, land.length, true);
    tiles.castShadow = false;
    const m = new THREE.Matrix4();
    const color = new THREE.Color();
    land.forEach((t, i) => {
      const [x, z] = this.tileXZ[t];
      tiles.setMatrixAt(i, m.makeTranslation(x, TILE_HEIGHT / 2, z));
      tiles.setColorAt(i, color.setHex(TERRAIN_COLORS[terrain[t]]));
    });
    this.commit(tiles, land.length);
    this.board.add(tiles);
    this.disposables.push(tileMaterial, tiles);
    if (sea.length) {
      const ringMaterial = new THREE.MeshBasicMaterial({ color: TERRAIN_COLORS[SEA_TERRAIN], transparent: true, opacity: 0.55, depthWrite: false });
      const rings = this.instanced(seaRingGeometry, ringMaterial, sea.length);
      rings.castShadow = rings.receiveShadow = false;
      sea.forEach((t, i) => rings.setMatrixAt(i, m.makeTranslation(this.tileXZ[t][0], -0.12, this.tileXZ[t][1])));
      this.commit(rings, sea.length);
      this.board.add(rings);
      this.disposables.push(ringMaterial, rings);
    }

    this.addDecorations(terrain);
    this.addTokens(numbers);
    this.addHarbors(harbors);
    this.requestRender();
  }

  /**
   * Updates roads, buildings and the robber. Pieces that were not there
   * before drop in; a moved robber walks over. Pass `animate: false` for the
   * first sync of a game.
   */
  setPieces({ buildings, roads, ships, robber, pirate }: PieceData, animate = true) {
    const now = animate ? performance.now() : -1e9;
    const nextShips = new Map<number, Piece>();
    (ships ?? []).forEach((owner, e) => {
      if (owner === EMPTY) return;
      const prev = this.shipPieces.get(e);
      const { x, z, angle } = this.edgeXZ[e];
      // Coastal ships sit off the beach, bow along the edge.
      const [ox, oz] = this.edgeSeaward[e] ?? [0, 0];
      const coastal = ox !== 0 || oz !== 0;
      const at = { x: x + ox * (coastal ? 0.42 : 0), z: z + oz * (coastal ? 0.42 : 0), angle };
      nextShips.set(e, prev?.owner === owner ? prev : { ...at, owner, born: now });
    });
    this.shipPieces = nextShips;

    if (pirate === undefined || pirate === null) {
      this.pirate.visible = false;
      this.pirateFrom = this.pirateTo = null;
    } else {
      const [px, pz] = this.tileXZ[pirate] ?? [0, 0];
      const to: [number, number] = [px, pz];
      const moved = this.pirateTo !== null && (to[0] !== this.pirateTo[0] || to[1] !== this.pirateTo[1]);
      if (animate && moved) {
        this.pirateFrom = [this.pirate.position.x, this.pirate.position.z];
        this.pirateStart = now;
      } else {
        this.pirateFrom = to;
        this.pirateStart = -1e9;
      }
      this.pirateTo = to;
      this.pirate.visible = true;
    }
    const nextRoads = new Map<number, Piece>();
    roads.forEach((owner, e) => {
      if (owner === EMPTY) return;
      const prev = this.roadPieces.get(e);
      const { x, z, angle } = this.edgeXZ[e];
      nextRoads.set(e, prev?.owner === owner ? prev : { x, z, angle, owner, born: now });
    });
    this.roadPieces = nextRoads;

    const nextBuildings = new Map<number, Piece & { city: boolean }>();
    buildings.forEach((b, v) => {
      if (b === EMPTY) return;
      const owner = b & ~CITY_FLAG;
      const city = (b & CITY_FLAG) !== 0;
      const prev = this.buildingPieces.get(v);
      const [x, z] = this.vertexXZ[v];
      // Face buildings towards the island centre.
      const angle = -Math.atan2(z, x);
      nextBuildings.set(v, prev && prev.owner === owner && prev.city === city ? prev : { x, z, angle, owner, city, born: now });
    });
    this.buildingPieces = nextBuildings;

    const [rx, rz] = this.tileXZ[robber] ?? [0, 0];
    const to: [number, number] = [rx + 0.42, rz + 0.12];
    if (animate && (to[0] !== this.robberTo[0] || to[1] !== this.robberTo[1]) && this.robberFrom !== null) {
      this.robberFrom = [this.robber.position.x, this.robber.position.z];
      this.robberStart = now;
    } else {
      this.robberFrom = [to[0], to[1]];
      this.robberStart = -1e9;
    }
    this.robberTo = to;
    this.updatePieces(performance.now());
    this.requestRender();
  }

  /** Keeps the island inside the screen area not covered by HUD panels. */
  setInsets(insets: Insets) {
    const same = (Object.keys(insets) as (keyof Insets)[]).every((k) => Math.abs(insets[k] - this.insets[k]) < 1);
    if (same) return;
    this.insets = { ...insets };
    this.resize();
  }

  /** Shows clickable markers on the given spots (or clears them). */
  setHighlights(h: Highlights | null) {
    this.highlights = h && h.ids.length ? h : null;
    this.hovered = -1;
    this.container.style.cursor = '';
    this.updateMarkers();
    this.requestRender();
  }

  /** Screen position (CSS pixels, relative to the container) of a spot. */
  project(kind: PickKind, id: number): { x: number; y: number } {
    const [x, z] =
      kind === 'tile' ? this.tileXZ[id] : kind === 'vertex' ? this.vertexXZ[id] : [this.edgeXZ[id].x, this.edgeXZ[id].z];
    const v = new THREE.Vector3(x, TOP + 0.1, z).project(this.camera);
    return { x: ((v.x + 1) / 2) * this.container.clientWidth, y: ((1 - v.y) / 2) * this.container.clientHeight };
  }

  requestRender() {
    if (this.renderQueued) return;
    this.renderQueued = true;
    requestAnimationFrame((t) => {
      this.renderQueued = false;
      const animating = this.updatePieces(t);
      this.controls.update(); // emits 'change' while damping, which queues the next frame
      this.renderer.render(this.scene, this.camera);
      if (animating) this.requestRender();
    });
  }

  dispose() {
    this.container.removeEventListener('pointermove', this.onPointerMove);
    this.container.removeEventListener('pointerdown', this.onPointerDown);
    this.container.removeEventListener('pointerup', this.onPointerUp);
    this.container.removeEventListener('pointerleave', this.onPointerLeave);
    this.resizeObserver.disconnect();
    this.controls.dispose();
    for (const d of this.disposables) d.dispose();
    for (const t of this.numberTextures.values()) t.dispose();
    this.renderer.dispose();
    this.renderer.domElement.remove();
  }

  // ---- input ----

  private spotUnderPointer(ev: PointerEvent): number {
    const h = this.highlights;
    if (!h) return -1;
    const rect = this.container.getBoundingClientRect();
    const ndc = new THREE.Vector2(((ev.clientX - rect.left) / rect.width) * 2 - 1, -((ev.clientY - rect.top) / rect.height) * 2 + 1);
    this.raycaster.setFromCamera(ndc, this.camera);
    const hit = this.raycaster.ray.intersectPlane(this.plane, new THREE.Vector3());
    if (!hit) return -1;
    let best = -1;
    let bestDist = PICK_RADIUS[h.kind];
    for (const id of h.ids) {
      const [x, z] = h.kind === 'tile' ? this.tileXZ[id] : h.kind === 'vertex' ? this.vertexXZ[id] : [this.edgeXZ[id].x, this.edgeXZ[id].z];
      const d = Math.hypot(hit.x - x, hit.z - z);
      if (d < bestDist) {
        bestDist = d;
        best = id;
      }
    }
    return best;
  }

  private onPointerMove = (ev: PointerEvent) => {
    if (!this.highlights || ev.pointerType === 'touch') return;
    const id = this.spotUnderPointer(ev);
    if (id !== this.hovered) {
      this.hovered = id;
      this.container.style.cursor = id >= 0 ? 'pointer' : '';
      this.updateMarkers();
      this.requestRender();
    }
  };

  private onPointerDown = (ev: PointerEvent) => {
    this.pointerDown = { x: ev.clientX, y: ev.clientY, t: performance.now() };
  };

  private onPointerUp = (ev: PointerEvent) => {
    const down = this.pointerDown;
    this.pointerDown = null;
    if (!down || !this.highlights) return;
    // A drag rotates the camera; only short, still presses count as clicks.
    if (Math.hypot(ev.clientX - down.x, ev.clientY - down.y) > 8 || performance.now() - down.t > 600) return;
    const id = this.spotUnderPointer(ev);
    if (id >= 0) this.onPick?.(this.highlights.kind, id);
  };

  private onPointerLeave = () => {
    if (this.hovered !== -1) {
      this.hovered = -1;
      this.container.style.cursor = '';
      this.updateMarkers();
      this.requestRender();
    }
  };

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

  /** Writes piece matrices for time `now`; returns true while animating. */
  private updatePieces(now: number): boolean {
    let animating = false;
    const m = new THREE.Matrix4();
    const q = new THREE.Quaternion();
    const s = new THREE.Vector3();
    const p = new THREE.Vector3();
    const up = new THREE.Vector3(0, 1, 0);
    const color = new THREE.Color();

    const place = (mesh: THREE.InstancedMesh, i: number, piece: Piece, y: number) => {
      const t = Math.min(1, (now - piece.born) / DROP_MS);
      if (t < 1) animating = true;
      const k = Math.max(0.001, easeOutBack(t));
      q.setFromAxisAngle(up, piece.angle);
      s.set(k, k, k);
      p.set(piece.x, y + (1 - Math.min(1, t * 1.4)) * 1.2, piece.z);
      mesh.setMatrixAt(i, m.compose(p, q, s));
      mesh.setColorAt(i, color.setHex(PLAYER_COLORS[piece.owner]));
    };

    let n = 0;
    for (const piece of this.roadPieces.values()) place(this.roads, n++, piece, TOP + 0.045);
    this.commit(this.roads, n);
    let nsh = 0;
    for (const piece of this.shipPieces.values()) place(this.ships, nsh++, piece, WATER);
    this.commit(this.ships, nsh);
    if (this.pirateTo) {
      const tp = Math.min(1, (now - this.pirateStart) / ROBBER_MS);
      const from = this.pirateFrom ?? this.pirateTo;
      const ep = easeInOut(tp);
      this.pirate.position.set(
        from[0] + (this.pirateTo[0] - from[0]) * ep,
        WATER + Math.sin(Math.PI * tp) * 0.25,
        from[1] + (this.pirateTo[1] - from[1]) * ep,
      );
      this.pirate.rotation.y = 0.6;
      if (tp < 1) animating = true;
    }
    let ns = 0;
    let nc = 0;
    for (const piece of this.buildingPieces.values()) {
      if (piece.city) place(this.cities, nc++, piece, TOP);
      else place(this.settlements, ns++, piece, TOP);
    }
    this.commit(this.settlements, ns);
    this.commit(this.cities, nc);

    const t = Math.min(1, (now - this.robberStart) / ROBBER_MS);
    const from = this.robberFrom ?? this.robberTo;
    const e = easeInOut(t);
    this.robber.position.set(
      from[0] + (this.robberTo[0] - from[0]) * e,
      TOP + Math.sin(Math.PI * t) * 0.9,
      from[1] + (this.robberTo[1] - from[1]) * e,
    );
    if (t < 1) animating = true;
    return animating;
  }

  private updateMarkers() {
    for (const mesh of Object.values(this.markers)) mesh.count = 0;
    const h = this.highlights;
    if (!h) return;
    const mesh = this.markers[h.kind];
    const m = new THREE.Matrix4();
    const q = new THREE.Quaternion();
    const s = new THREE.Vector3();
    const p = new THREE.Vector3();
    const up = new THREE.Vector3(0, 1, 0);
    const color = new THREE.Color();
    h.ids.forEach((id, i) => {
      const hover = id === this.hovered;
      const k = hover ? 1.35 : 1;
      if (h.kind === 'vertex') {
        const [x, z] = this.vertexXZ[id];
        p.set(x, TOP + 0.03, z);
        q.identity();
      } else if (h.kind === 'edge') {
        const { x, z, angle } = this.edgeXZ[id];
        p.set(x, TOP + 0.03, z);
        q.setFromAxisAngle(up, angle);
      } else {
        const [x, z] = this.tileXZ[id];
        p.set(x, TOP + 0.02, z);
        q.identity();
      }
      s.set(k, 1, k);
      mesh.setMatrixAt(i, m.compose(p, q, s));
      mesh.setColorAt(i, color.setHex(hover ? ACCENT : 0xffffff));
    });
    this.commit(mesh, h.ids.length);
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

  private addDecorations(terrain: number[]) {
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

  private addTokens(numbers: number[]) {
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

  private addHarbors(harbors: number[]) {
    const plankMaterial = std(0x8a5a35);
    const planks = this.instanced(new THREE.BoxGeometry(1, 0.05, 0.09), plankMaterial, harbors.length * 2);
    const docks = this.instanced(new THREE.BoxGeometry(0.34, 0.08, 0.34), plankMaterial, harbors.length);
    this.disposables.push(plankMaterial, planks, docks, planks.geometry, docks.geometry);
    const m = new THREE.Matrix4();
    const q = new THREE.Quaternion();
    const s = new THREE.Vector3();
    const p = new THREE.Vector3();
    const up = new THREE.Vector3(0, 1, 0);
    const topology = this.topology!;
    const ev = topology.edgeVertices;

    harbors.forEach((code, slot) => {
      const e = topology.harborEdges[slot];
      const a = this.vertexXZ[ev[2 * e]];
      const b = this.vertexXZ[ev[2 * e + 1]];
      const mx = (a[0] + b[0]) / 2;
      const mz = (a[1] + b[1]) / 2;
      const [ox, oz] = this.harborOut[slot];
      const dock: [number, number] = [mx + ox * 0.5, mz + oz * 0.5];
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
    // Fit the island into the free area (at least 45% of the screen each way)
    // and shift the projection centre into the middle of that area.
    const { top, right, bottom, left } = this.insets;
    const safeW = Math.max(w * 0.45, w - left - right);
    const safeH = Math.max(h * 0.45, h - top - bottom);
    const tanV = Math.tan(THREE.MathUtils.degToRad(this.camera.fov / 2));
    const tanH = tanV * this.camera.aspect;
    const distance = Math.max(this.extent.x / (tanH * (safeW / w)), this.extent.z / (tanV * (safeH / h)));
    const shiftX = Math.min(left, w - safeW) + safeW / 2 - w / 2;
    const shiftY = Math.min(top, h - safeH) + safeH / 2 - h / 2;
    this.camera.setViewOffset(w, h, -shiftX, -shiftY, w, h);
    // Keep the user's viewing angle and relative zoom across refits.
    const current = this.camera.position.length();
    const zoom = this.fitDistance > 0 && current > 0 ? current / this.fitDistance : 1;
    const dir = current > 0 && this.fitDistance > 0 ? this.camera.position.clone().normalize() : CAMERA_DIR;
    this.fitDistance = distance;
    this.camera.position.copy(dir).multiplyScalar(distance * zoom);
    this.controls.minDistance = distance * 0.45;
    this.controls.maxDistance = distance * 1.6;
    const fog = this.scene.fog as THREE.Fog;
    fog.near = Math.max(22, distance * 1.6);
    fog.far = Math.max(60, distance * 4);
    this.camera.updateProjectionMatrix();
    this.controls.update();
    this.requestRender();
  }
}
