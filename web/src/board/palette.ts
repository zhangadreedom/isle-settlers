// Original low-poly palette for the island. Terrain codes match isle-core's
// `Terrain` enum; resource codes match `Resource`.

export const TERRAIN_COLORS = [
  0xc0623a, // hills (brick)
  0x2f7d4a, // forest (lumber)
  0x8cc56b, // pasture (wool)
  0xe2b74a, // fields (grain)
  0x8a8f99, // mountains (ore)
  0xe5d3a1, // desert
  0x2a8199, // sea (seafaring maps: shallow water rings)
  0x7d5f36, // gold field (dark river valley; nuggets carry the gold)
  0x8eb0c6, // fog (unexplored misty sea)
] as const;

export const RESOURCE_COLORS = [0xc0623a, 0x2f7d4a, 0xb9e08f, 0xe2b74a, 0x8a8f99] as const;
export const RESOURCE_LABELS = ['砖', '木', '羊', '麦', '矿'] as const;

export const COMMODITY_COLORS = [0xd8c394, 0xb06ee8, 0xf0be32] as const;
export const METROPOLIS_COLORS = [0xffca3a, 0x5aa9ff, 0x3ed694] as const;

export const PLAYER_COLORS = [0xd94841, 0x3b7dd8, 0xf2efe6, 0xf08c00, 0x8e5bd6, 0x22b8c9] as const;
export const PLAYER_CSS = ['#d94841', '#3b7dd8', '#f2efe6', '#f08c00', '#8e5bd6', '#22b8c9'] as const;

export const SEA = 0x1f6f8b;
export const SKY = 0x0d3b52;
export const BEACH = 0xd9c48f;

export interface ThemeLightingConfig {
  sky: number;
  fog: number;
  fogMinNear: number;
  fogNearFactor: number;
  fogMinFar: number;
  fogFarFactor: number;
  sea: number;
  hemiSky: number;
  hemiGround: number;
  hemiIntensity: number;
  sunColor: number;
  sunIntensity: number;
  sunPos: readonly [number, number, number];
  rimColor: number;
  rimIntensity: number;
  rimPos: readonly [number, number, number];
}

export const THEME_CONFIGS = {
  day: {
    sky: SKY,
    fog: SKY,
    fogMinNear: 22,
    fogNearFactor: 1.6,
    fogMinFar: 60,
    fogFarFactor: 4,
    sea: SEA,
    hemiSky: 0xdff3ff,
    hemiGround: 0x2d4a3a,
    hemiIntensity: 1.3,
    sunColor: 0xfff1d6,
    sunIntensity: 2.4,
    sunPos: [-6, 12, 5],
    rimColor: 0x5b8cff,
    rimIntensity: 0,
    rimPos: [7, 6, -6],
  },
  sunset: {
    sky: 0x2b1d38,
    fog: 0x3b2439,
    fogMinNear: 18,
    fogNearFactor: 1.35,
    fogMinFar: 50,
    fogFarFactor: 3.4,
    sea: 0x16384c,
    hemiSky: 0xffb88c,
    hemiGround: 0x2a1f33,
    hemiIntensity: 1.15,
    sunColor: 0xff9e5e,
    sunIntensity: 2.6,
    sunPos: [-9, 7.5, 6],
    rimColor: 0x5b8cff,
    rimIntensity: 0,
    rimPos: [7, 6, -6],
  },
  night: {
    sky: 0x08131f,
    fog: 0x08131f,
    fogMinNear: 20,
    fogNearFactor: 1.5,
    fogMinFar: 56,
    fogFarFactor: 3.8,
    sea: 0x0a2233,
    hemiSky: 0x6fa8dc,
    hemiGround: 0x101c28,
    hemiIntensity: 0.85,
    sunColor: 0xffe0a3,
    sunIntensity: 2.1,
    sunPos: [-4, 11, 4],
    rimColor: 0x5b8cff,
    rimIntensity: 0.65,
    rimPos: [7, 6, -6],
  },
} as const satisfies Record<'day' | 'sunset' | 'night', ThemeLightingConfig>;

