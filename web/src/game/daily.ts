// Deterministic Daily Seed Challenge generator, safe localStorage persistence,
// and spoiler-free share card formatter.

import type { Scenario } from '../generated/proto';

export interface DailyChallengeSpec {
  dateKey: string;
  seed: number;
  scenario: Scenario;
  players: number;
  bot: 'greedy' | 'mcts';
  friendlyRobber: boolean;
  victoryPoints: number;
  title: string;
  subtitle: string;
}

export interface DailyAttemptRecord {
  dateKey: string;
  won: boolean;
  turns: number;
  points: number;
  attempts: number;
  bestTurns: number | null;
}

const DAILY_STORAGE_KEY = 'isle.daily.records';
const MAX_STORED_DAYS = 90;

/** Rotating pool across days of the week. */
const DAILY_SCENARIO_ROTATION: readonly Scenario[] = [
  'Classic',
  'Archipelago',
  'FogIslands',
  'Citadels',
  'Fishermen',
  'Caravans',
];

interface ScenarioMeta {
  name: string;
  defaultVp: number;
  tagline: string;
}

/**
 * Metadata keyed by `string` so both existing scenarios and future additions
 * (such as `'Fishermen'`) resolve cleanly without brittle exhaustive-record breaks.
 */
const SCENARIO_META: Record<string, ScenarioMeta> = {
  Classic: {
    name: '经典本岛',
    defaultVp: 10,
    tagline: '六角本岛资源博弈与商路竞速',
  },
  Archipelago: {
    name: '蔚蓝群岛',
    defaultVp: 12,
    tagline: '扬帆跨越海峡，登陆外岛赢取开拓高分',
  },
  FogIslands: {
    name: '迷雾群岛',
    defaultVp: 12,
    tagline: '探索未知迷雾海域，翻开隐秘富饶群岛',
  },
  Citadels: {
    name: '铁卫城邦',
    defaultVp: 13,
    tagline: '二轮起城产出商品，升级大都会并共御黑帆',
  },
  Fishermen: {
    name: '丰饶渔港',
    defaultVp: 10,
    tagline: '经营沿岸渔场与内陆湖泊，巧用鲜鱼奇策',
  },
  Caravans: {
    name: '绿洲驼商',
    defaultVp: 12,
    tagline: '引导绿洲骆驼商队穿村加分，跨黄金河造桥积累金币',
  },
};

const WEEKDAY_PREFIXES = ['每日远征', '每日开拓', '每日航程', '每日争锋'] as const;
const WEEKEND_PREFIXES = ['周末王冠', '周末远征', '周末决战', '周末争霸'] as const;
const WEEKDAY_NAMES = ['周日', '周一', '周二', '周三', '周四', '周五', '周六'] as const;

/** In-memory fallback so sandboxed embeds retain results during the session. */
const memoryRecords = new Map<string, DailyAttemptRecord>();

export function scenarioDisplayName(scenario: string): string {
  return SCENARIO_META[scenario]?.name ?? scenario;
}

export function scenarioDefaultVp(scenario: string): number {
  return SCENARIO_META[scenario]?.defaultVp ?? 10;
}

/** Returns local calendar date formatted as `YYYY-MM-DD`. */
export function getTodayKey(now: Date = new Date()): string {
  const year = now.getFullYear();
  const month = String(now.getMonth() + 1).padStart(2, '0');
  const day = String(now.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

/**
 * Deterministic 32-bit FNV-1a + MurmurHash3 finalizer over `"isle-daily:" + dateKey`.
 * Produces a positive 31-bit integer seed (`1..2147483647`).
 */
export function hashDailyKey(dateKey: string): number {
  const key = `isle-daily:${dateKey}`;
  let h = 0x811c9dc5;
  for (let i = 0; i < key.length; i++) {
    h ^= key.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  // Murmur3 avalanche mixing so adjacent calendar dates spread uniformly.
  h ^= h >>> 16;
  h = Math.imul(h, 0x85ebca6b);
  h ^= h >>> 13;
  h = Math.imul(h, 0xc2b2ae35);
  h ^= h >>> 16;
  return (h >>> 1) || 1;
}

function parseDateParts(dateKey: string): { year: number; month: number; day: number } {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(dateKey.trim());
  if (match) {
    const year = Number(match[1]);
    const month = Number(match[2]);
    const day = Number(match[3]);
    if (month >= 1 && month <= 12 && day >= 1 && day <= 31) {
      return { year, month, day };
    }
  }
  const now = new Date();
  return { year: now.getFullYear(), month: now.getMonth() + 1, day: now.getDate() };
}

/** Deterministic daily challenge specification for `dateKey` (defaults to today). */
export function getDailyChallenge(dateKey: string = getTodayKey()): DailyChallengeSpec {
  const seed = hashDailyKey(dateKey);
  const { year, month, day } = parseDateParts(dateKey);
  const localDate = new Date(year, month - 1, day);
  const dow = localDate.getDay(); // 0 = Sun .. 6 = Sat
  const isWeekend = dow === 0 || dow === 6;

  // Epoch day index rotates cleanly across consecutive days and cycles weekend scenarios across weeks.
  const epochDay = Math.floor(Date.UTC(year, month - 1, day) / 86_400_000);
  const rotIdx = ((epochDay % DAILY_SCENARIO_ROTATION.length) + DAILY_SCENARIO_ROTATION.length) % DAILY_SCENARIO_ROTATION.length;
  const scenario = DAILY_SCENARIO_ROTATION[rotIdx];
  const meta = SCENARIO_META[scenario] ?? {
    name: String(scenario),
    defaultVp: 10,
    tagline: '同种子公平竞技挑战',
  };

  const bot: 'greedy' | 'mcts' = isWeekend ? 'mcts' : 'greedy';
  const players = 4;
  const friendlyRobber = true;
  const victoryPoints = meta.defaultVp;

  const prefixPool = isWeekend ? WEEKEND_PREFIXES : WEEKDAY_PREFIXES;
  const prefix = prefixPool[seed % prefixPool.length];
  const title = `${prefix} · ${meta.name}`;
  const dayLabel = WEEKDAY_NAMES[dow] ?? '今日';
  const botLabel = bot === 'mcts' ? '困难 MCTS AI' : '普通贪心 AI';
  const subtitle = `${dayLabel}${isWeekend ? '大师赛' : '同图赛'} · ${meta.tagline}（${botLabel} · ${victoryPoints}分获胜）`;

  return {
    dateKey,
    seed,
    scenario,
    players,
    bot,
    friendlyRobber,
    victoryPoints,
    title,
    subtitle,
  };
}

function sanitizeRecord(raw: unknown): DailyAttemptRecord | null {
  if (!raw || typeof raw !== 'object') return null;
  const r = raw as Partial<DailyAttemptRecord>;
  if (typeof r.dateKey !== 'string' || !r.dateKey) return null;
  if (typeof r.won !== 'boolean') return null;
  if (typeof r.turns !== 'number' || !Number.isFinite(r.turns) || r.turns < 0) return null;
  if (typeof r.points !== 'number' || !Number.isFinite(r.points) || r.points < 0) return null;
  if (typeof r.attempts !== 'number' || !Number.isFinite(r.attempts) || r.attempts < 1) return null;
  const bestTurns =
    typeof r.bestTurns === 'number' && Number.isFinite(r.bestTurns) && r.bestTurns > 0
      ? Math.round(r.bestTurns)
      : null;
  return {
    dateKey: r.dateKey,
    won: r.won,
    turns: Math.round(r.turns),
    points: Math.round(r.points),
    attempts: Math.round(r.attempts),
    bestTurns,
  };
}

function readStoredMap(): Record<string, DailyAttemptRecord> {
  const out: Record<string, DailyAttemptRecord> = {};
  for (const [k, v] of memoryRecords.entries()) {
    out[k] = v;
  }
  try {
    if (typeof localStorage === 'undefined') return out;
    const text = localStorage.getItem(DAILY_STORAGE_KEY);
    if (!text) return out;
    const parsed: unknown = JSON.parse(text);
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return out;
    for (const [k, v] of Object.entries(parsed as Record<string, unknown>)) {
      const clean = sanitizeRecord(v);
      if (clean && clean.dateKey === k) {
        out[k] = clean;
        memoryRecords.set(k, clean);
      }
    }
  } catch {
    // Sandboxed iframe (`dist-embed`) or corrupt JSON: fall back to session map.
  }
  return out;
}

function writeStoredMap(records: Record<string, DailyAttemptRecord>): void {
  const keys = Object.keys(records).sort();
  const trimmedKeys = keys.length > MAX_STORED_DAYS ? keys.slice(keys.length - MAX_STORED_DAYS) : keys;
  const bounded: Record<string, DailyAttemptRecord> = {};
  for (const k of trimmedKeys) {
    bounded[k] = records[k];
    memoryRecords.set(k, records[k]);
  }
  try {
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem(DAILY_STORAGE_KEY, JSON.stringify(bounded));
    }
  } catch {
    // Storage disabled or quota exceeded in sandboxed embeds; memory fallback remains active.
  }
}

export function getDailyRecord(dateKey: string): DailyAttemptRecord | null {
  const map = readStoredMap();
  return map[dateKey] ?? null;
}

export function recordDailyResult(
  dateKey: string,
  won: boolean,
  turns: number,
  points: number,
): DailyAttemptRecord {
  const safeTurns = Math.max(1, Math.round(Number.isFinite(turns) ? turns : 1));
  const safePoints = Math.max(0, Math.round(Number.isFinite(points) ? points : 0));
  const map = readStoredMap();
  const prev = map[dateKey];

  let bestTurns = prev?.bestTurns ?? null;
  if (won) {
    bestTurns = bestTurns === null ? safeTurns : Math.min(bestTurns, safeTurns);
  }

  const nextWon = prev ? prev.won || won : won;
  const nextPoints = prev
    ? won && !prev.won
      ? safePoints
      : Math.max(prev.points, safePoints)
    : safePoints;

  const updated: DailyAttemptRecord = {
    dateKey,
    won: nextWon,
    turns: safeTurns,
    points: nextPoints,
    attempts: (prev?.attempts ?? 0) + 1,
    bestTurns,
  };

  map[dateKey] = updated;
  writeStoredMap(map);
  return updated;
}

/** Crisp, spoiler-free share card string for clipboard sharing. */
export function formatDailyShareText(spec: DailyChallengeSpec, rec: DailyAttemptRecord): string {
  const resultLine = rec.won
    ? `🏆 获胜！用时 ${rec.bestTurns ?? rec.turns} 回合 · ${rec.points} 分`
    : `⚔️ 惜败（第 ${rec.turns} 回合 · ${rec.points} 分）`;
  const attemptNote = rec.attempts > 1 ? ` · 共 ${rec.attempts} 次尝试` : '';
  return `🏝️ Isle Settlers 每日挑战 ${spec.dateKey}\n📜 ${spec.title}（种子 #${spec.seed}）\n${resultLine}${attemptNote}`;
}
