// Original low-poly palette for the island. Terrain codes match isle-core's
// `Terrain` enum; resource codes match `Resource`.

export const TERRAIN_COLORS = [
  0xc0623a, // hills (brick)
  0x2f7d4a, // forest (lumber)
  0x8cc56b, // pasture (wool)
  0xe2b74a, // fields (grain)
  0x8a8f99, // mountains (ore)
  0xe5d3a1, // desert
] as const;

export const RESOURCE_COLORS = [0xc0623a, 0x2f7d4a, 0xb9e08f, 0xe2b74a, 0x8a8f99] as const;
export const RESOURCE_LABELS = ['砖', '木', '羊', '麦', '矿'] as const;

export const PLAYER_COLORS = [0xd94841, 0x3b7dd8, 0xf2efe6, 0xf08c00, 0x8e5bd6, 0x22b8c9] as const;
export const PLAYER_CSS = ['#d94841', '#3b7dd8', '#f2efe6', '#f08c00', '#8e5bd6', '#22b8c9'] as const;

export const SEA = 0x1f6f8b;
export const SKY = 0x0d3b52;
export const BEACH = 0xd9c48f;
