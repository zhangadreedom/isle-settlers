import { createSignal, type Accessor } from 'solid-js';

export type Locale = 'zh' | 'en';

export interface BilingualText {
  readonly zh: string;
  readonly en: string;
}

export const LANG_STORAGE_KEY = 'isle.lang';

function syncDocumentLang(next: Locale): void {
  try {
    if (typeof document !== 'undefined' && document.documentElement) {
      document.documentElement.lang = next === 'en' ? 'en' : 'zh-CN';
    }
  } catch {
    // Ignore headless / non-DOM environments.
  }
}

function readStoredLocale(): Locale {
  try {
    if (typeof window !== 'undefined') {
      const raw = window.localStorage.getItem(LANG_STORAGE_KEY);
      if (raw === 'en' || raw === 'zh') return raw;
    }
  } catch {
    // Sandboxed iframes (`dist-embed`) throw SecurityError on localStorage access.
  }
  return 'zh';
}

function writeStoredLocale(next: Locale): void {
  try {
    if (typeof window !== 'undefined') {
      window.localStorage.setItem(LANG_STORAGE_KEY, next);
    }
  } catch {
    // Ignore blocked storage in sandboxed iframes or private contexts.
  }
}

const initialLocale = readStoredLocale();
syncDocumentLang(initialLocale);

const [localeSignal, setLocaleSignal] = createSignal<Locale>(initialLocale);

/** Reactive accessor for the active UI locale (`'zh'` default, or `'en'`). */
export const locale: Accessor<Locale> = localeSignal;

/** Sets active locale, persists to `localStorage` (`'isle.lang'`), and updates `<html lang>`. */
export function setLocale(next: Locale): void {
  const normalized: Locale = next === 'en' ? 'en' : 'zh';
  setLocaleSignal(normalized);
  writeStoredLocale(normalized);
  syncDocumentLang(normalized);
}

/** Toggles between `'zh'` and `'en'` and returns the new locale. */
export function toggleLocale(): Locale {
  const next: Locale = localeSignal() === 'zh' ? 'en' : 'zh';
  setLocale(next);
  return next;
}

/** Reactive inline bilingual helper for Solid JSX and memos. */
export function t(zh: string, en: string): string {
  return localeSignal() === 'en' ? en : zh;
}

/** Reactive selector for a `{ zh, en }` dictionary entry. */
export function tr(entry: BilingualText): string {
  return localeSignal() === 'en' ? entry.en : entry.zh;
}

// ---------- Domain Keys & Lookup Dictionaries ----------

export type ScenarioKey =
  | 'Classic'
  | 'Archipelago'
  | 'FogIslands'
  | 'Citadels'
  | 'Fishermen'
  | 'Caravans'
  | 'Explorers';

export const SCENARIOS: readonly ScenarioKey[] = [
  'Classic',
  'Archipelago',
  'FogIslands',
  'Citadels',
  'Fishermen',
  'Caravans',
  'Explorers',
];

export const SCENARIO_TITLE: Record<ScenarioKey, BilingualText> = {
  Classic: { zh: '经典', en: 'Classic' },
  Archipelago: { zh: '航海 · 群岛', en: 'Seafarers · Archipelago' },
  FogIslands: { zh: '航海 · 迷雾', en: 'Seafarers · Fog Islands' },
  Citadels: { zh: '城邦与铁卫', en: 'Citadels & Knights' },
  Fishermen: { zh: '渔场与丰年', en: 'Fishermen & Plenty' },
  Caravans: { zh: '商队与桥梁', en: 'Caravans & Bridges' },
  Explorers: { zh: '探索与海盗', en: 'Explorers & Pirates' },
};

export const SCENARIO_SHORT: Record<ScenarioKey, BilingualText> = {
  Classic: { zh: '经典', en: 'Classic' },
  Archipelago: { zh: '群岛', en: 'Isles' },
  FogIslands: { zh: '迷雾', en: 'Fog' },
  Citadels: { zh: '城邦', en: 'Citadels' },
  Fishermen: { zh: '渔场', en: 'Fishery' },
  Caravans: { zh: '商队', en: 'Caravans' },
  Explorers: { zh: '远洋', en: 'Explorers' },
};

export const SCENARIO_BLURB: Record<ScenarioKey, BilingualText> = {
  Classic: {
    zh: '在六边形岛屿上开拓、交易、建造，先到 10 分者获胜。5–6 人时使用扩展岛屿，支持传统特殊建造或双人搭档回合。',
    en: 'Settle, trade, and build across the hex island — first to 10 VP wins. 5–6 players use the extended island with Special Build or Paired Turns.',
  },
  Archipelago: {
    zh: '航海 · 群岛：从西边的主岛出发，造船驶过海峡，首次在东边外岛建村庄奖励 2 分；金矿任选资源，海盗封锁航线。先到 12 分获胜，支持 2–6 人。',
    en: 'Seafarers · Archipelago: Set sail from the western home island across the strait — earn +2 bonus VP for each new eastern island settled, pick any resource from Gold fields, and watch out for the Pirate. 12 VP to win (2–6 players).',
  },
  FogIslands: {
    zh: '航海 · 迷雾：从月牙主岛扬帆驶入迷雾，航线触及未知格即翻开地形并获发现奖励，首次登陆迷雾新大陆奖励 2 分。先到 12 分获胜，支持 2–6 人。',
    en: 'Seafarers · Fog Islands: Sail from the crescent home island into uncharted fog — reveal hidden hexes for instant discovery rewards and earn +2 VP when settling newly discovered islands. 12 VP to win (2–6 players).',
  },
  Citadels: {
    zh: '城邦与铁卫：第二轮直接起城，城市产出造纸/织布/铸币升级三系科技与 4 分大都会；招募并激活实体铁卫抵御黑帆舰队入侵。先到 13 分获胜，支持 2–6 人。',
    en: 'Citadels & Knights: Place a starting city in round 2, produce Paper/Cloth/Coin to upgrade Trade, Politics, and Science tracks toward 4-VP Metropolises, and rally active Knights against the Black Sail fleet. 13 VP to win (2–6 players).',
  },
  Fishermen: {
    zh: '渔场与丰年：沿海渔场与荒漠内陆湖产出海鱼筹码，花 2–7 条鱼可驱匪、抽牌、换资源、修路或摸卡，小心捞到 +1 分难度的破旧皮靴！先到 10 分获胜，支持 2–6 人。',
    en: 'Fishermen & Plenty: Harvest fish tokens from coastal fishing grounds and the inland lake; spend 2–7 fish to drive off the robber, steal, claim bank resources, build free roads, or draw dev cards — beware the +1 VP Old Boot! 10 VP to win (2–6 players).',
  },
  Caravans: {
    zh: '商队与桥梁：架设跨河桥梁贯通水系，新建聚落与升级城市推进绿洲骆驼商队延伸，商队途经的村城与跨河贸易网均可斩获额外胜分！支持 2–6 人。',
    en: 'Caravans & Bridges: Span inland rivers with bridges, extend camel caravan routes from the central oasis as settlements and cities rise, and earn bonus VP along connected caravan trade routes! Supports 2–6 players.',
  },
  Explorers: {
    zh: '探索与海盗：扬帆远航推进【围剿海盗巢穴 / 远洋渔运 / 香料外岛盟约】三大任务链（每阶+1分、满阶夺王冠），掷骰轮空自动补贴 +1 金币！先到 15 分获胜，支持 2–6 人。',
    en: 'Explorers & Pirates: Sail across the archipelago to raid Pirate Lairs, haul Deep Fish Shoals, and forge Spice Trade pacts (+1 VP per step + Champion crowns), with +1 compensation gold on dry rolls! 15 VP to win (2–6 players).',
  },
};

export type ResourceKey = 'Brick' | 'Lumber' | 'Wool' | 'Grain' | 'Ore';

export const RESOURCE_LABEL: Record<ResourceKey, BilingualText> = {
  Brick: { zh: '砖', en: 'Brick' },
  Lumber: { zh: '木', en: 'Lumber' },
  Wool: { zh: '羊毛', en: 'Wool' },
  Grain: { zh: '小麦', en: 'Grain' },
  Ore: { zh: '矿石', en: 'Ore' },
};

export const RESOURCE_SHORT_LABEL: Record<ResourceKey, BilingualText> = {
  Brick: { zh: '砖', en: 'Br' },
  Lumber: { zh: '木', en: 'Lu' },
  Wool: { zh: '羊', en: 'Wo' },
  Grain: { zh: '麦', en: 'Gr' },
  Ore: { zh: '矿', en: 'Or' },
};

export type CommodityKey = 'Paper' | 'Cloth' | 'Coin';

export const COMMODITY_LABEL: Record<CommodityKey, BilingualText> = {
  Paper: { zh: '造纸', en: 'Paper' },
  Cloth: { zh: '织布', en: 'Cloth' },
  Coin: { zh: '铸币', en: 'Coin' },
};

export const COMMODITY_SHORT_LABEL: Record<CommodityKey, BilingualText> = {
  Paper: { zh: '纸', en: 'Pa' },
  Cloth: { zh: '布', en: 'Cl' },
  Coin: { zh: '币', en: 'Co' },
};

export type TerrainKey =
  | 'Hills'
  | 'Forest'
  | 'Pasture'
  | 'Fields'
  | 'Mountains'
  | 'Desert'
  | 'Sea'
  | 'Gold'
  | 'Fog'
  | 'Oasis';

export const TERRAIN_LABEL: Record<TerrainKey, BilingualText> = {
  Hills: { zh: '丘陵', en: 'Hills' },
  Forest: { zh: '森林', en: 'Forest' },
  Pasture: { zh: '牧场', en: 'Pasture' },
  Fields: { zh: '农田', en: 'Fields' },
  Mountains: { zh: '山脉', en: 'Mountains' },
  Desert: { zh: '沙漠', en: 'Desert' },
  Sea: { zh: '海面', en: 'Sea' },
  Gold: { zh: '金矿', en: 'Gold Field' },
  Fog: { zh: '迷雾', en: 'Fog' },
  Oasis: { zh: '绿洲', en: 'Oasis' },
};

export type BotPersonaKey =
  | 'Balanced'
  | 'RoadBuilder'
  | 'CityBaron'
  | 'Warlord'
  | 'Merchant';

export const BOT_PERSONAS: readonly BotPersonaKey[] = [
  'Balanced',
  'RoadBuilder',
  'CityBaron',
  'Warlord',
  'Merchant',
];

export const BOT_PERSONA_LABEL: Record<BotPersonaKey, BilingualText> = {
  Balanced: { zh: '均衡全能', en: 'Balanced' },
  RoadBuilder: { zh: '修路狂魔', en: 'Road Pioneer' },
  CityBaron: { zh: '筑城富豪', en: 'City Baron' },
  Warlord: { zh: '铁卫统帅', en: 'Iron Warlord' },
  Merchant: { zh: '热心皇商', en: 'Keen Merchant' },
};

export const BOT_PERSONA_BLURB: Record<BotPersonaKey, BilingualText> = {
  Balanced: {
    zh: '稳健评估资源、扩建与控场，无明显偏科的全能型对手',
    en: 'Weighs expansion, upgrades, and board control evenly across all phases',
  },
  RoadBuilder: {
    zh: '偏爱砖木与航道要津，积极抢占最长商路、跨海航线与桥梁优势',
    en: 'Prioritizes Brick & Lumber to race Longest Road, sea lanes, and river bridges',
  },
  CityBaron: {
    zh: '偏爱麦矿重镇，速升城市、大都会科技与发展卡爆发得分',
    en: 'Focuses on Grain & Ore to rush Cities, Metropolises, and Development Cards',
  },
  Warlord: {
    zh: '崇尚武力控场，高频招募铁卫、打出骑士并封锁领跑者核心产地',
    en: 'Aggressively deploys Knights and blocks the leader’s highest-yield hexes',
  },
  Merchant: {
    zh: '紧盯港口、金矿、渔场与商队，热衷高频贸易周转手牌',
    en: 'Secures harbors, Gold fields, fisheries, and caravans for rapid trade turnover',
  },
};

export const UI_TEXT = {
  // Lobby & top-bar tools
  dailyChallenge: { zh: '每日挑战', en: 'Daily' },
  mapWorkshop: { zh: '地图工坊', en: 'Workshop' },
  rules: { zh: '规则', en: 'Rules' },
  shortcuts: { zh: '快捷键', en: 'Shortcuts' },
  shortcutsAndA11y: { zh: '快捷键与无障碍设置', en: 'Shortcuts & Accessibility' },
  installApp: { zh: '安装应用', en: 'Install App' },
  themeDay: { zh: '晴日', en: 'Day' },
  themeSunset: { zh: '黄昏', en: 'Sunset' },
  themeNight: { zh: '星夜', en: 'Night' },
  soundOn: { zh: '音效', en: 'Sound' },
  soundMuted: { zh: '静音', en: 'Muted' },
  colorblindOn: { zh: '纹章开', en: 'Symbols On' },
  colorblindOff: { zh: '纹章关', en: 'Symbols Off' },

  // Lobby tabs & form fields
  tabLocal: { zh: '单机 · 对战 AI', en: 'Solo vs AI' },
  tabOnline: { zh: '联机对战', en: 'Online' },
  tabRank: { zh: '排行榜', en: 'Leaderboard' },
  fieldScenario: { zh: '玩法', en: 'Scenario' },
  fieldPlayers: { zh: '人数', en: 'Players' },
  fieldAiDifficulty: { zh: 'AI 难度', en: 'AI Level' },
  fieldAiPersona: { zh: 'AI 性格', en: 'AI Persona' },
  aiEasy: { zh: '简单', en: 'Easy' },
  aiNormal: { zh: '普通', en: 'Normal' },
  aiHard: { zh: '困难', en: 'Hard' },
  houseRulesSummary: { zh: '自定义房规与计时', en: 'House Rules & Timer' },
  fieldVictoryPoints: { zh: '胜利分', en: 'Target VP' },
  defaultOption: { zh: '默认', en: 'Default' },
  fieldFriendlyRobber: { zh: '友好强盗', en: 'Friendly Robber' },
  standardOption: { zh: '标准', en: 'Standard' },
  friendlyRobberOn: { zh: '开启（保护 ≤2 分）', en: 'On (Protect ≤2 VP)' },
  fieldDiceMode: { zh: '点数产生', en: 'Dice Mode' },
  standardDice: { zh: '标准双骰', en: 'Standard 2d6' },
  balancedDeck: { zh: '36张均衡牌组', en: '36-Card Deck' },
  fieldFiveSixRule: { zh: '5–6人规则', en: '5–6P Rule' },
  specialBuildRule: { zh: '传统特殊建造', en: 'Special Build' },
  pairedTurnsRule: { zh: '双人搭档回合', en: 'Paired Turns' },
  fieldTurnTimer: { zh: '每步限时', en: 'Turn Timer' },
  noTimer: { zh: '不限时', en: 'Off' },
  fieldYourSeat: { zh: '你的座位', en: 'Your Seat' },
  randomSeat: { zh: '随机', en: 'Random' },
  firstSeat: { zh: '先手', en: '1st' },
  fieldPace: { zh: '节奏', en: 'Pace' },
  paceNormal: { zh: '正常', en: 'Normal' },
  paceFast: { zh: '快速', en: 'Fast' },
  fieldMap: { zh: '地图', en: 'Board' },
  rerollMap: { zh: '换一张', en: 'Reroll' },
  diyWorkshop: { zh: 'DIY 工坊', en: 'DIY Workshop' },
  diyCustomMap: { zh: 'DIY 自定义地图', en: 'DIY Custom Map' },
  edit: { zh: '编辑', en: 'Edit' },
  resetRandom: { zh: '恢复随机', en: 'Reset Random' },
  startGame: { zh: '开始游戏', en: 'Start Game' },
  spectateAi: { zh: '观战 AI 对局', en: 'Watch AI Match' },
  openReplayFile: { zh: '打开回放文件…', en: 'Open Replay File…' },
  createRoom: { zh: '创建房间', en: 'Create Room' },
  joinRoom: { zh: '加入房间', en: 'Join Room' },

  // In-game HUD & controls
  backToLobby: { zh: '← 大厅', en: '← Lobby' },
  leaveGame: { zh: '离开', en: 'Leave' },
  gameLog: { zh: '对局记录', en: 'Game Log' },
  bank: { zh: '银行', en: 'Bank' },
  rollDice: { zh: '掷骰子', en: 'Roll Dice' },
  buildRoad: { zh: '道路', en: 'Road' },
  buildSettlement: { zh: '村庄', en: 'Settlement' },
  buildCity: { zh: '城市', en: 'City' },
  buildShip: { zh: '船', en: 'Ship' },
  moveShip: { zh: '移船', en: 'Move Ship' },
  buildBridge: { zh: '桥', en: 'Bridge' },
  buyDevCard: { zh: '发展卡', en: 'Dev Card' },
  trade: { zh: '交易', en: 'Trade' },
  endTurn: { zh: '结束回合', en: 'End Turn' },
  endPairedTurn: { zh: '结束副回合', en: 'End Paired Turn' },
  passSpecialBuild: { zh: '跳过', en: 'Pass' },
  citadelTech: { zh: '城邦科技', en: 'City Tech' },
  fishMarket: { zh: '鱼市', en: 'Fish Market' },
  longestRoad: { zh: '最长道路', en: 'Longest Road' },
  longestTradeRoute: { zh: '最长商路', en: 'Longest Route' },
  largestArmy: { zh: '最大骑士', en: 'Largest Army' },
  close: { zh: '关闭', en: 'Close' },
  cancel: { zh: '取消', en: 'Cancel' },
  confirm: { zh: '确认', en: 'Confirm' },
  playReplay: { zh: '播放', en: 'Play' },
  pauseReplay: { zh: '暂停', en: 'Pause' },
  replayAgain: { zh: '重播', en: 'Replay' },
  watchReplay: { zh: '观看回放', en: 'Watch Replay' },
  postGameStats: { zh: '数据复盘', en: 'Match Stats' },
} as const satisfies Record<string, BilingualText>;

export type UiTextKey = keyof typeof UI_TEXT;

// ---------- Reactive Lookup Helpers ----------

function lookupRecord(dict: Record<string, BilingualText>, key: string): string {
  const entry = dict[key];
  if (!entry) return key;
  return localeSignal() === 'en' ? entry.en : entry.zh;
}

export function scenarioTitle(scenario: ScenarioKey | (string & {})): string {
  return lookupRecord(SCENARIO_TITLE, scenario);
}

export function scenarioShort(scenario: ScenarioKey | (string & {})): string {
  return lookupRecord(SCENARIO_SHORT, scenario);
}

export function scenarioBlurb(scenario: ScenarioKey | (string & {})): string {
  return lookupRecord(SCENARIO_BLURB, scenario);
}

export function resourceLabel(resource: ResourceKey | (string & {})): string {
  return lookupRecord(RESOURCE_LABEL, resource);
}

export function resourceShortLabel(resource: ResourceKey | (string & {})): string {
  return lookupRecord(RESOURCE_SHORT_LABEL, resource);
}

export function commodityLabel(commodity: CommodityKey | (string & {})): string {
  return lookupRecord(COMMODITY_LABEL, commodity);
}

export function commodityShortLabel(commodity: CommodityKey | (string & {})): string {
  return lookupRecord(COMMODITY_SHORT_LABEL, commodity);
}

export function terrainLabel(terrain: TerrainKey | (string & {})): string {
  return lookupRecord(TERRAIN_LABEL, terrain);
}

export function botPersonaLabel(persona: BotPersonaKey | (string & {})): string {
  return lookupRecord(BOT_PERSONA_LABEL, persona);
}

export function botPersonaBlurb(persona: BotPersonaKey | (string & {})): string {
  return lookupRecord(BOT_PERSONA_BLURB, persona);
}

export function ui(key: UiTextKey): string {
  return tr(UI_TEXT[key]);
}
