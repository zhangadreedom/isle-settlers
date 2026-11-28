import { createMemo, createSignal, For, Show } from 'solid-js';
import { PLAYER_CSS } from '../board/palette';
import { Replay } from '../engine';
import { analyzeReplayRecord } from '../game/stats';
import type { Action, CityTrack, Event, Phase, PlayerView, ReplayRecord } from '../generated/proto';
import { resourceLabel, t } from '../i18n';
import { SeatDot } from './bits';

export interface WinProbPoint {
  turn: number;
  probs: number[];
}

export interface TurningPointInfo {
  actionIndex: number;
  turn: number;
  player: number;
  action: Action;
  beforePct: number;
  afterPct: number;
  deltaPct: number;
  kind: string;
}

export interface ReplayAnalysisData {
  winSeries: WinProbPoint[];
  turningPoints: TurningPointInfo[];
}

const DICE_SUMS = [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12] as const;
const CALLOUT_GLYPHS = ['①', '②', '③'] as const;

function diceBarColor(sum: number): string {
  if (sum === 6 || sum === 8) return '#d94841';
  if (sum === 7) return '#c98a2c';
  return '#2f9aa8';
}

function clamp(val: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, val));
}

function isSetupPhase(phase: Phase): boolean {
  return (
    phase.type === 'SetupSettlement' ||
    phase.type === 'SetupRoad' ||
    (phase.type === 'Gold' && phase.value.vertex !== null)
  );
}

function softmaxProbs(scores: readonly number[], targetVp: number, numPlayers: number): number[] {
  const denom = Math.max(1, targetVp);
  const temp = 6.0;
  const logits: number[] = [];
  for (let p = 0; p < numPlayers; p++) {
    const vp = scores[p] ?? 0;
    const ratio = vp / denom;
    const closeness = vp >= denom ? 2.5 : vp >= denom - 1 ? 0.35 : 0;
    logits.push(ratio * temp + closeness);
  }
  const maxLogit = Math.max(...logits);
  const exps = logits.map((l) => Math.exp(l - maxLogit));
  const sumExp = exps.reduce((a, b) => a + b, 0) || 1;
  return exps.map((e) => e / sumExp);
}

function oneHotWinner(winner: number, numPlayers: number): number[] {
  return Array.from({ length: numPlayers }, (_, s) => (s === winner ? 1 : 0));
}

function classifyActionStep(
  actor: number,
  action: Action,
  events: readonly Event[],
  prevProbs: readonly number[],
  nextProbs: readonly number[],
): { player: number; kind: string } {
  for (const e of events) {
    if (e.type === 'CityPillaged') {
      return { player: e.value.player, kind: 'BarbarianRaid' };
    }
  }
  for (const e of events) {
    if (e.type === 'LongestRoadChanged' && e.value.owner !== null) {
      return { player: e.value.owner, kind: 'LongestRoad' };
    }
    if (e.type === 'LargestArmyChanged' && e.value.owner !== null) {
      return { player: e.value.owner, kind: 'LargestArmy' };
    }
    if (e.type === 'CityUpgraded' && e.value.metropolis !== null) {
      return { player: e.value.player, kind: 'Metropolis' };
    }
  }
  for (const e of events) {
    if (e.type === 'IslandSettled' || e.type === 'TileRevealed') {
      return { player: e.value.player, kind: 'ExplorerMission' };
    }
    if (e.type === 'CamelPlaced' || e.type === 'BridgeBuilt') {
      return { player: e.value.player, kind: 'CamelCaravan' };
    }
    if (e.type === 'MonopolyTaken') {
      return { player: e.value.player, kind: 'Monopoly' };
    }
    if (e.type === 'BarbariansRepelled' && e.value.sole_defender !== null) {
      return { player: e.value.sole_defender, kind: 'BarbarianRaid' };
    }
    if (e.type === 'CityBuilt') {
      return { player: e.value.player, kind: 'CityBuilt' };
    }
    if (e.type === 'SettlementBuilt') {
      return { player: e.value.player, kind: 'SettlementBuilt' };
    }
  }

  if (
    action.type === 'RollDice' ||
    action.type === 'Discard' ||
    action.type === 'DiscardCitadel' ||
    action.type === 'MoveRobber'
  ) {
    let bestSeat = actor;
    let bestAbs = -1;
    for (let p = 0; p < nextProbs.length; p++) {
      const diff = (nextProbs[p] ?? 0) - (prevProbs[p] ?? 0);
      const abs = Math.abs(diff);
      if (abs > bestAbs + 1e-6 || (Math.abs(abs - bestAbs) <= 1e-6 && diff > 0)) {
        bestAbs = abs;
        bestSeat = p;
      }
    }
    return { player: bestSeat, kind: 'DiceSwing' };
  }

  if (action.type === 'PlayMonopoly') return { player: actor, kind: 'Monopoly' };
  if (action.type === 'PlaceCamel' || action.type === 'BuildBridge') {
    return { player: actor, kind: 'CamelCaravan' };
  }
  return { player: actor, kind: 'TacticalPlay' };
}

function scanReplayTurningPoints(
  record: ReplayRecord,
  numPlayers: number,
  targetVp: number,
): TurningPointInfo[] | null {
  let replay: Replay | null = null;
  try {
    replay = new Replay(JSON.stringify(record));
    const r = replay;
    const evalProbs = (specView: PlayerView): number[] => {
      if (specView.winner !== null && specView.winner >= 0 && specView.winner < numPlayers) {
        return oneHotWinner(specView.winner, numPlayers);
      }
      const scores: number[] = [];
      for (let p = 0; p < numPlayers; p++) {
        const trueVp = r.view(p).me?.victoryPoints ?? specView.players[p]?.publicVictoryPoints ?? 0;
        const pub = specView.players[p];
        const cards = ((pub?.resourceCount ?? 0) + (pub?.commodityCount ?? 0) * 1.3) * 0.04;
        const tech =
          ((pub?.cityProgress[0] ?? 0) + (pub?.cityProgress[1] ?? 0) + (pub?.cityProgress[2] ?? 0)) * 0.07;
        const extras =
          (pub?.fishCount ?? 0) * 0.035 +
          (pub?.coins ?? 0) * 0.045 +
          (specView.oldBootOwner === p ? -0.75 : 0);
        scores.push(trueVp + cards + tech + extras);
      }
      return softmaxProbs(scores, targetVp, numPlayers);
    };

    let curView = r.view(null);
    let setupDone = !isSetupPhase(curView.phase);
    let prevProbs = setupDone
      ? evalProbs(curView)
      : Array.from({ length: numPlayers }, () => 1 / numPlayers);

    const candidates: TurningPointInfo[] = [];

    for (;;) {
      const step = r.step(null);
      if (!step) break;
      const actionIndex = r.position;
      curView = r.view(null);

      if (!setupDone) {
        if (!isSetupPhase(curView.phase)) {
          setupDone = true;
          prevProbs = evalProbs(curView);
        }
        continue;
      }

      const nextProbs = evalProbs(curView);
      const { player, kind } = classifyActionStep(
        step.player,
        step.action,
        step.events,
        prevProbs,
        nextProbs,
      );
      const beforePct = (prevProbs[player] ?? 0) * 100;
      const afterPct = (nextProbs[player] ?? 0) * 100;
      const deltaPct = afterPct - beforePct;
      prevProbs = nextProbs;

      if (step.action.type === 'EndTurn' && Math.abs(deltaPct) < 1) continue;
      if (Math.abs(deltaPct) < 1.5) continue;

      candidates.push({
        actionIndex,
        turn: curView.turn,
        player,
        action: step.action,
        beforePct,
        afterPct,
        deltaPct,
        kind,
      });
    }

    candidates.sort((a, b) => Math.abs(b.deltaPct) - Math.abs(a.deltaPct));
    const picked: TurningPointInfo[] = [];
    const seenTurnSeat = new Set<string>();
    for (const c of candidates) {
      const key = `${c.turn}:${c.player}:${c.kind}`;
      if (seenTurnSeat.has(key)) continue;
      seenTurnSeat.add(key);
      picked.push(c);
      if (picked.length >= 3) break;
    }
    return picked;
  } catch {
    return null;
  } finally {
    replay?.free();
  }
}

/**
 * Pure-TS fallback estimator computing `ReplayAnalysisData` from `vpTimeline`
 * (softmax over `points / targetVp`), ensuring `PostGameStats` never fails on
 * older runtimes when `Replay.analyze()` / `props.analysis` is unavailable.
 */
export function estimateReplayAnalysis(
  record: ReplayRecord,
  vpTimeline: readonly { turn: number; points: number[] }[],
  totalActions = 0,
): ReplayAnalysisData {
  const numPlayers = Math.max(2, record.config.numPlayers || 2);
  const targetVp = Math.max(1, record.config.victoryPoints || 10);
  const winner = record.winner;
  const evenProbs = Array.from({ length: numPlayers }, () => 1 / numPlayers);

  let winSeries: WinProbPoint[];
  if (vpTimeline.length === 0) {
    winSeries = [{ turn: 0, probs: evenProbs }];
  } else if (vpTimeline.length === 1) {
    const only = vpTimeline[0]!;
    const startProbs = softmaxProbs(only.points, targetVp, numPlayers);
    if (winner !== null && winner >= 0 && winner < numPlayers) {
      winSeries = [
        { turn: 0, probs: startProbs },
        { turn: Math.max(1, only.turn), probs: oneHotWinner(winner, numPlayers) },
      ];
    } else {
      winSeries = [{ turn: only.turn, probs: startProbs }];
    }
  } else {
    winSeries = vpTimeline.map((row, idx) => {
      const isLast = idx === vpTimeline.length - 1;
      if (isLast && winner !== null && winner >= 0 && winner < numPlayers) {
        return { turn: row.turn, probs: oneHotWinner(winner, numPlayers) };
      }
      return {
        turn: row.turn,
        probs: softmaxProbs(row.points, targetVp, numPlayers),
      };
    });
  }

  const scanned = scanReplayTurningPoints(record, numPlayers, targetVp);
  if (scanned && scanned.length > 0) {
    return { winSeries, turningPoints: scanned };
  }

  // Pure timeline-only fallback (works even without WASM loaded).
  const maxTurn = Math.max(1, winSeries[winSeries.length - 1]?.turn ?? 1);
  const fallbackSwings: TurningPointInfo[] = [];
  for (let i = 1; i < winSeries.length; i++) {
    const prev = winSeries[i - 1]!;
    const curr = winSeries[i]!;
    const prevPts = vpTimeline[i - 1]?.points ?? [];
    const currPts = vpTimeline[i]?.points ?? [];

    let bestSeat = 0;
    let bestAbs = -1;
    let bestDelta = 0;
    for (let p = 0; p < numPlayers; p++) {
      const b = (prev.probs[p] ?? 0) * 100;
      const a = (curr.probs[p] ?? 0) * 100;
      const d = a - b;
      const abs = Math.abs(d);
      if (abs > bestAbs + 1e-6 || (Math.abs(abs - bestAbs) <= 1e-6 && d > bestDelta)) {
        bestAbs = abs;
        bestDelta = d;
        bestSeat = p;
      }
    }
    if (bestAbs < 1.5) continue;

    const vpDiff = (currPts[bestSeat] ?? 0) - (prevPts[bestSeat] ?? 0);
    let kind = 'TacticalPlay';
    let action: Action = { type: 'EndTurn' };
    if (vpDiff >= 2) {
      kind = record.config.scenario === 'Citadels' ? 'Metropolis' : 'LongestRoad';
      action =
        record.config.scenario === 'Citadels'
          ? { type: 'UpgradeCity', value: { track: 'Science', metropolis: 0 } }
          : { type: 'BuildRoad', value: 0 };
    } else if (vpDiff === 1) {
      kind = 'CityBuilt';
      action = { type: 'BuildCity', value: 0 };
    } else if (vpDiff < 0) {
      kind = record.config.scenario === 'Citadels' ? 'BarbarianRaid' : 'LongestRoad';
      action = { type: 'RollDice' };
    }

    const estActionIdx =
      totalActions > 0 ? Math.max(1, Math.round((curr.turn / maxTurn) * totalActions)) : i;

    fallbackSwings.push({
      actionIndex: estActionIdx,
      turn: curr.turn,
      player: bestSeat,
      action,
      beforePct: (prev.probs[bestSeat] ?? 0) * 100,
      afterPct: (curr.probs[bestSeat] ?? 0) * 100,
      deltaPct: bestDelta,
      kind,
    });
  }

  fallbackSwings.sort((a, b) => Math.abs(b.deltaPct) - Math.abs(a.deltaPct));
  return {
    winSeries,
    turningPoints: fallbackSwings.slice(0, 3),
  };
}

function trackLabel(track: CityTrack): string {
  switch (track) {
    case 'Trade':
      return t('商贸', 'Trade');
    case 'Politics':
      return t('军政', 'Politics');
    case 'Science':
      return t('学术', 'Science');
  }
}

function formatWinProbJump(pt: TurningPointInfo): string {
  const dFixed = pt.deltaPct.toFixed(0);
  const cleanDelta = dFixed === '-0' ? '0' : dFixed;
  const sign = pt.deltaPct >= 0 ? '+' : '';
  return `${pt.beforePct.toFixed(0)}% → ${pt.afterPct.toFixed(0)}% (${sign}${cleanDelta}%)`;
}

function describeTurningPoint(pt: TurningPointInfo): {
  tag: string;
  headline: string;
  takeaway: string;
} {
  const pos = pt.deltaPct >= 0;
  const act = pt.action;

  switch (pt.kind) {
    case 'LongestRoad': {
      const isSea = act.type === 'BuildShip' || act.type === 'MoveShip';
      const isRoadCard = act.type === 'PlayRoadBuilding';
      return {
        tag: isSea ? t('最长商路', 'Longest Route') : t('最长道路', 'Longest Road'),
        headline: pos
          ? isRoadCard
            ? t(
                '打出【道路建设】连拓双路，一举夺下【最长道路】（+2 分）！',
                'Played Road Building to connect key segments and seize Longest Road (+2 VP)!',
              )
            : isSea
              ? t(
                  '延伸跨海航线贯通商道，成功夺取【最长商路】（+2 分）！',
                  'Extended sea shipping lanes to claim Longest Trade Route (+2 VP)!',
                )
              : t(
                  '关键修路贯通主干网络，夺下【最长道路】2 分王牌！',
                  'Connected core road segments to capture Longest Road (+2 VP)!',
                )
          : t(
              '主干路网被对手超越或插村截断，痛失【最长道路】2 分优势。',
              'Lost Longest Road (-2 VP swing) after rival extension or settlement road-break.',
            ),
        takeaway: t(
          'AI 锦囊：领跑最长路时预留 1–2 段分支冗余，并留意对手可能插村腰斩的空交叉点。',
          'AI Tip: Keep 1–2 backup road branches and guard open intersections against rival road-breaks.',
        ),
      };
    }

    case 'LargestArmy':
      return {
        tag: t('最大骑士', 'Largest Army'),
        headline: pos
          ? t(
              '果断调遣骑士驱匪控场，加冕【最大骑士】（+2 分）并封锁对手高产地块！',
              'Deployed Knight to lock down rival production and claim Largest Army (+2 VP)!',
            )
          : t(
              '骑士出牌数被对手反超，让出【最大骑士】2 分与强盗控制权。',
              'Outpaced in knight count — surrendered Largest Army (-2 VP) and robber tempo.',
            ),
        takeaway: t(
          'AI 锦囊：当台面骑士逼近 3 张临界点时，提前储备发展卡以稳住军力王冠与产地安全。',
          'AI Tip: Watch face-up knight counts around the 3-card threshold and keep a reserve Knight ready.',
        ),
      };

    case 'Metropolis': {
      const trk = act.type === 'UpgradeCity' ? trackLabel(act.value.track) : t('城邦', 'Citadel');
      return {
        tag: t('大都会加冕', 'Metropolis'),
        headline: pos
          ? t(
              `升级${trk}科技至四阶以上加冕【大都会】（+2 分且免疫黑帆劫掠），奠定胜局！`,
              `Upgraded ${trk} track to crown a Metropolis (+2 VP & pillage-proof), seizing control!`,
            )
          : t(
              '对手率先冲满五阶科技，【大都会】冠冕易主导致分差剧变。',
              'A rival reached Level 5 first and snatched away the Metropolis crown.',
            ),
        takeaway: t(
          'AI 锦囊：四阶大都会仍可能被五阶夺走；商品充裕时宜一鼓作气升满五阶彻底锁分。',
          'AI Tip: Lv.4 Metropolises can be stolen at Lv.5 — push to Lv.5 early if rivals hoard commodities.',
        ),
      };
    }

    case 'ExplorerMission':
      return {
        tag: t('远航开拓', 'Island Discovery'),
        headline: pos
          ? t(
              '扬帆跨海登陆新岛或拨开迷雾新矿脉，斩获海外开拓奖励分与丰厚资源！',
              'Sailed into new waters to reveal fog or settle a new island for bonus VP and fresh yield!',
            )
          : t(
              '海外良港被对手抢先登陆，外岛扩张空间与登岛奖励受限。',
              'Beaten to prime outer-island landing spots, limiting overseas bonus VP.',
            ),
        takeaway: t(
          'AI 锦囊：航海与迷雾剧本中，尽早配置木羊造船出海，外岛 +2 分往往比内陆苦争更高效。',
          'AI Tip: Pair Lumber + Wool early in Seafarers scenarios — +2 VP outer islands accelerate victory.',
        ),
      };

    case 'CamelCaravan': {
      const isBridge = act.type === 'BuildBridge';
      return {
        tag: isBridge ? t('跨河建桥', 'River Bridge') : t('骆驼商队', 'Camel Caravan'),
        headline: pos
          ? isBridge
            ? t(
                '架设黄金河石拱桥斩获金币并打通两岸商路，商队贸易胜率大幅跃升！',
                'Built a river stone bridge for gold coins and linked lucrative caravan routes!',
              )
            : t(
                '精准引导绿洲骆驼商队穿过己方村城网络，激活连片商道额外胜分！',
                'Steered the oasis camel caravan across friendly settlements and cities for bonus VP!',
              )
          : t(
              '骆驼商队主干改道流向对手聚落群，错失商路加分红利。',
              'The camel caravan branched toward rival settlements, swinging trade-route VP.',
            ),
        takeaway: t(
          'AI 锦囊：优先卡住绿洲出口与跨河桥头堡，让后续每次商队延伸都顺势串联己方建筑。',
          'AI Tip: Secure oasis exits and river bridgeheads early so caravan votes naturally favor your network.',
        ),
      };
    }

    case 'Monopoly': {
      const resText = act.type === 'PlayMonopoly' ? resourceLabel(act.value) : null;
      return {
        tag: t('资源垄断', 'Monopoly'),
        headline: pos
          ? resText
            ? t(
                `精准打出【垄断】席卷全场【${resText}】，瞬间掏空对手并引爆连环建造！`,
                `Timed Monopoly on ${resText} to sweep rival hands and fuel an explosive build turn!`,
              )
            : t(
                '精准打出【垄断】席卷全场关键资源，一举改写经济与得分天平！',
                'Timed a devastating Monopoly sweep, flipping resource tempo across the board!',
              )
          : t(
              '遭遇【垄断】重创，手中囤积的核心资源被洗劫一空，升城扩建节奏中断。',
              'Caught holding stacked cards when Monopoly hit, stalling critical upgrades.',
            ),
        takeaway: t(
          'AI 锦囊：记住近期高频产出的资源流向择机收割；防守端切忌长期囤积单一已暴露资源。',
          'AI Tip: Track recent dice payouts before firing Monopoly, and convert surplus single-type resources fast.',
        ),
      };
    }

    case 'BarbarianRaid':
      return {
        tag: t('黑帆攻城', 'Barbarian Raid'),
        headline: pos
          ? t(
              '铁卫防线合力击退黑帆舰队，独揽【卫城勋章】（+1 分）并护住核心城市！',
              'Active Knights repelled the Black Sail fleet — earning Defender VP while saving cities!',
            )
          : t(
              '黑帆舰队攻破城防！铁卫战力不足导致城市沦陷降级为村庄，产能与胜率重挫。',
              'Black Sail raiders pillaged an undefended city back to a settlement — crippling production.',
            ),
        takeaway: t(
          'AI 锦囊：黑帆逼近至 5–6 格时务必预留小麦激活铁卫，切勿无防备盲目升城。',
          'AI Tip: Always keep Grain ready to activate Knights before the fleet reaches 6/7.',
        ),
      };

    case 'CityBuilt':
      return {
        tag: t('升级城市', 'City Built'),
        headline: pos
          ? t(
              '在核心高产交叉点升起【城市】（+1 分），资源产能翻倍开启滚雪球节奏！',
              'Upgraded a prime settlement to a City (+1 VP), doubling resource yield to snowball ahead!',
            )
          : t(
              '对手率先完成核心城市升级，中盘双倍产出差距拉开胜率分水岭。',
              'Rival completed a key City upgrade first, opening a mid-game production gap.',
            ),
        takeaway: t(
          'AI 锦囊：优先升级覆盖 6/8 高点数或麦矿三格交汇的村庄，单城翻倍即可盘活全局经济。',
          'AI Tip: Prioritize 6/8 high-pip or Grain+Ore 3-hex vertices for your first City upgrade.',
        ),
      };

    case 'SettlementBuilt':
      return {
        tag: t('新建村庄', 'Settlement'),
        headline: pos
          ? t(
              '抢先落子关键交叉点新建【村庄】（+1 分），拓宽资源品类并封堵对手出路！',
              'Claimed a contested vertex with a new Settlement (+1 VP), expanding income and blocking rivals!',
            )
          : t(
              '必争建村良点被对手抢先一步占据，外扩路线遭挤压。',
              'Beaten to a contested settlement spot by one tempo, restricting expansion lanes.',
            ),
        takeaway: t(
          'AI 锦囊：遇到双人修路竞速同一良点时，宁可借助港口/银行折价换牌也要抢先落地。',
          'AI Tip: In a vertex race, trade aggressively with harbors or bank to plant the settlement one turn sooner.',
        ),
      };

    case 'DiceSwing':
      return {
        tag: t('骰运波峰', 'Dice Swing'),
        headline: pos
          ? t(
              '高频骰点集中爆发（或 7 点精准控匪断粮），单轮资源雪崩推高胜率！',
              'High-impact dice surge (or a clutch 7 robber lock) generated a massive tempo lead!',
            )
          : t(
              '遭遇 7 点爆牌弃半或主产地被强盗死锁，关键回合手牌断档导致胜率滑坡。',
              'Punished by a 7-roll half-hand discard or prolonged robber blockade on core hexes.',
            ),
        takeaway: t(
          'AI 锦囊：手牌超过 7 张前尽量买发展卡、修路或挂单换整，降低掷 7 爆牌风险。',
          'AI Tip: Spend hands down to 7 or fewer via dev cards, roads, or 2:1 harbors to dodge 7-roll discards.',
        ),
      };

    case 'TacticalPlay':
    default: {
      let actionDetailZh = '打出一波关键战术连招，高效转化手牌资源扩大盘面胜势！';
      let actionDetailEn = 'Executed a decisive tactical turn, converting resources into lasting board advantage!';
      if (act.type === 'PlayYearOfPlenty') {
        actionDetailZh = '打出【丰收】精准补齐紧缺双资源，当回合完成关键建造破局！';
        actionDetailEn = 'Played Year of Plenty to snipe two missing resources and unlock an immediate build!';
      } else if (act.type === 'PlayRoadBuilding') {
        actionDetailZh = '打出【道路建设】连铺两段要道，瞬间抢占版图制高点！';
        actionDetailEn = 'Played Road Building for two instant segments, seizing prime board territory!';
      } else if (act.type === 'BuyDevCard' || act.type === 'FishBuyDevCard') {
        actionDetailZh = '关键时机购入发展卡补充暗分或骑士储备，稳步锁定胜局！';
        actionDetailEn = 'Drew a timely Development Card, bolstering hidden VP or knight control!';
      } else if (
        act.type === 'MaritimeTrade' ||
        act.type === 'MaritimeTradeGood' ||
        act.type === 'ConfirmTrade'
      ) {
        actionDetailZh = '通过精妙港口/玩家贸易盘活卡手余牌，抢出关键得分节奏！';
        actionDetailEn = 'Leveraged smart harbor/player trading to convert surplus cards into decisive VP!';
      } else if (act.type === 'PassOldBoot') {
        actionDetailZh = '巧妙转交【破旧皮靴】，卸下 +1 分获胜枷锁并拖慢领跑对手！';
        actionDetailEn = 'Passed the Old Boot penalty to a rival, shedding the +1 VP victory burden!';
      } else if (
        act.type === 'FishDriveOffRobber' ||
        act.type === 'FishSteal' ||
        act.type === 'FishTakeResource' ||
        act.type === 'FishBuildRoad'
      ) {
        actionDetailZh = '灵活消耗海鱼筹码兑现鱼市奇策，无损手牌抢下关键先手！';
        actionDetailEn = 'Spent fish tokens at the Fish Market for a zero-card tempo swing!';
      } else if (act.type === 'SpendCoinsResource') {
        actionDetailZh = '动用黄金河金币精准购入缺口资源，一锤定音完成冲分！';
        actionDetailEn = 'Spent gold coins for the exact missing resource to clinch key points!';
      } else if (!pos) {
        actionDetailZh = '此步局势天平显著倾斜，对手趁势拉开盘面领先身位。';
        actionDetailEn = 'Momentum shifted sharply here as rivals capitalized on board tempo.';
      }
      return {
        tag: t('战术博弈', 'Tactical Play'),
        headline: t(actionDetailZh, actionDetailEn),
        takeaway: t(
          'AI 锦囊：善用港口折算与发展卡回合连招，在单一回合内把散牌一次性兑现为稳固胜分。',
          'AI Tip: Chain harbor trades with dev cards in a single turn to turn idle cards into permanent VP.',
        ),
      };
    }
  }
}

function smoothPolylinePoints(pts: readonly { x: number; y: number }[], minY: number, maxY: number): string {
  if (pts.length === 0) return '';
  if (pts.length === 1) return `${pts[0]!.x.toFixed(1)},${pts[0]!.y.toFixed(1)}`;
  const subdivisions = pts.length <= 36 ? 4 : 2;
  const out: string[] = [`${pts[0]!.x.toFixed(1)},${pts[0]!.y.toFixed(1)}`];
  for (let i = 0; i < pts.length - 1; i++) {
    const a = pts[i]!;
    const b = pts[i + 1]!;
    if (Math.abs(b.x - a.x) < 0.8) {
      out.push(`${b.x.toFixed(1)},${clamp(b.y, minY, maxY).toFixed(1)}`);
      continue;
    }
    for (let s = 1; s <= subdivisions; s++) {
      const u = s / subdivisions;
      const mu = (1 - Math.cos(u * Math.PI)) * 0.5;
      const x = a.x + u * (b.x - a.x);
      const y = clamp(a.y + mu * (b.y - a.y), minY, maxY);
      out.push(`${x.toFixed(1)},${y.toFixed(1)}`);
    }
  }
  return out.join(' ');
}

function interpolateSeatPct(
  winSeries: readonly WinProbPoint[],
  turn: number,
  seat: number,
  probScale: number,
  fallbackPct: number,
): number {
  if (winSeries.length === 0) return clamp(fallbackPct, 0, 100);
  const first = winSeries[0]!;
  if (turn <= first.turn) return clamp((first.probs[seat] ?? 0) * probScale, 0, 100);
  const last = winSeries[winSeries.length - 1]!;
  if (turn >= last.turn) return clamp((last.probs[seat] ?? 0) * probScale, 0, 100);

  for (let i = 1; i < winSeries.length; i++) {
    const prev = winSeries[i - 1]!;
    const curr = winSeries[i]!;
    if (turn >= prev.turn && turn <= curr.turn) {
      const span = curr.turn - prev.turn;
      const p0 = (prev.probs[seat] ?? 0) * probScale;
      const p1 = (curr.probs[seat] ?? 0) * probScale;
      const ratio = span > 0 ? (turn - prev.turn) / span : 1;
      return clamp(p0 + ratio * (p1 - p0), 0, 100);
    }
  }
  return clamp(fallbackPct, 0, 100);
}

export function PostGameStats(props: {
  record: ReplayRecord;
  name: (seat: number) => string;
  analysis?: ReplayAnalysisData | null;
  onSeekReplay?: (record: ReplayRecord, actionIndex: number) => void;
}) {
  const stats = createMemo(() => {
    try {
      return analyzeReplayRecord(props.record);
    } catch {
      return null;
    }
  });
  const [tab, setTab] = createSignal<'dice' | 'vp' | 'winprob' | 'econ'>('dice');
  const isCitadels = () => props.record.config.scenario === 'Citadels';

  const analysisData = createMemo<ReplayAnalysisData>(() => {
    if (props.analysis) {
      if (props.analysis.winSeries.length > 0) return props.analysis;
      const stFallback = stats();
      const est = estimateReplayAnalysis(
        props.record,
        stFallback?.vpTimeline ?? [],
        stFallback?.totalActions ?? 0,
      );
      return {
        winSeries: est.winSeries,
        turningPoints:
          props.analysis.turningPoints.length > 0 ? props.analysis.turningPoints : est.turningPoints,
      };
    }
    // Probe runtime `Replay.prototype.analyze` if exposed by the WASM host.
    type ReplayWithAnalyze = Replay & { analyze?: () => ReplayAnalysisData };
    const proto = Replay.prototype as ReplayWithAnalyze;
    if (typeof proto.analyze === 'function') {
      let replay: ReplayWithAnalyze | null = null;
      try {
        replay = new Replay(JSON.stringify(props.record)) as ReplayWithAnalyze;
        const res = replay.analyze!();
        if (res && Array.isArray(res.winSeries) && res.winSeries.length > 0) {
          return res;
        }
      } catch {
        // Fall through to deterministic TS estimator.
      } finally {
        replay?.free();
      }
    }
    const st = stats();
    return estimateReplayAnalysis(props.record, st?.vpTimeline ?? [], st?.totalActions ?? 0);
  });

  const winProbGeometry = createMemo(() => {
    const data = analysisData();
    const st = stats();
    const numPlayers = Math.max(2, props.record.config.numPlayers || 2);
    const padL = 34;
    const padR = 14;
    const topY = 18;
    const baseY = 136;
    const plotW = 360 - padL - padR;
    const plotH = baseY - topY;
    const midY = topY + plotH / 2;

    const rawMax = Math.max(0, ...data.winSeries.flatMap((pt) => pt.probs));
    const probScale = rawMax <= 1.05 ? 100 : 1;

    const seriesLastTurn = data.winSeries[data.winSeries.length - 1]?.turn ?? 0;
    const tpMaxTurn = Math.max(0, ...data.turningPoints.slice(0, 3).map((tp) => tp.turn));
    const maxTurn = Math.max(1, seriesLastTurn, tpMaxTurn, st?.totalTurns ?? 0);

    const xOf = (turn: number) => padL + (clamp(turn, 0, maxTurn) / maxTurn) * plotW;
    const yOf = (pct: number) => baseY - (clamp(pct, 0, 100) / 100) * plotH;

    const series = Array.from({ length: numPlayers }, (_, seat) => {
      const rawPts =
        data.winSeries.length === 0
          ? [
              { x: padL, y: yOf(100 / numPlayers) },
              { x: padL + plotW, y: yOf(100 / numPlayers) },
            ]
          : data.winSeries.length === 1
            ? [
                { x: padL, y: yOf((data.winSeries[0]!.probs[seat] ?? 0) * probScale) },
                { x: padL + plotW, y: yOf((data.winSeries[0]!.probs[seat] ?? 0) * probScale) },
              ]
            : data.winSeries.map((row) => ({
                x: xOf(row.turn),
                y: yOf((row.probs[seat] ?? 0) * probScale),
              }));
      const lastPt = rawPts[rawPts.length - 1] ?? { x: padL, y: baseY };
      const lastRow = data.winSeries[data.winSeries.length - 1];
      const finalPct = lastRow ? clamp((lastRow.probs[seat] ?? 0) * probScale, 0, 100) : 100 / numPlayers;

      return {
        seat,
        color: PLAYER_CSS[seat] ?? '#eef4f6',
        isWinner: props.record.winner === seat,
        polyline: smoothPolylinePoints(rawPts, topY, baseY),
        lastX: lastPt.x,
        lastY: lastPt.y,
        finalPct,
      };
    });

    const placedBadges: { cx: number; badgeY: number }[] = [];
    const callouts = data.turningPoints.slice(0, 3).map((pt, idx) => {
      const glyph = CALLOUT_GLYPHS[idx] ?? '①';
      const cx = xOf(pt.turn);
      const linePct = interpolateSeatPct(data.winSeries, pt.turn, pt.player, probScale, pt.afterPct);
      const anchorY = yOf(linePct);
      const preferredY = clamp(anchorY - 12, topY + 6, baseY - 8);
      let badgeY = preferredY;
      for (const offset of [0, 18, -18, 36, -36]) {
        const candidate = clamp(preferredY + offset, topY + 6, baseY - 8);
        const collides = placedBadges.some(
          (prev) => Math.abs(prev.cx - cx) < 16 && Math.abs(prev.badgeY - candidate) < 16,
        );
        if (!collides) {
          badgeY = candidate;
          break;
        }
      }
      placedBadges.push({ cx, badgeY });
      return {
        idx,
        glyph,
        pt,
        cx,
        anchorY,
        badgeY,
        seatColor: PLAYER_CSS[pt.player] ?? '#eef4f6',
      };
    });

    return {
      padL,
      padR,
      topY,
      midY,
      baseY,
      plotW,
      maxTurn,
      series,
      callouts,
      topTurningPoints: data.turningPoints.slice(0, 3),
    };
  });

  return (
    <Show when={stats() || props.analysis}>
      {(_) => {
        const activeTab = () => (!stats() && tab() !== 'winprob' ? 'winprob' : tab());

        const diceGeometry = createMemo(() => {
          const st = stats();
          const padL = 14;
          const padR = 14;
          const baseY = 124;
          const chartH = 98;
          const slotW = (360 - padL - padR) / DICE_SUMS.length;
          const barW = 19;
          if (!st) {
            return { padL, padR, baseY, items: [], expectedPolyline: '' };
          }
          const maxVal = Math.max(
            1,
            ...DICE_SUMS.map((n) => Math.max(st.diceCounts[n] ?? 0, st.expectedCounts[n] ?? 0)),
          );
          const items = DICE_SUMS.map((n, idx) => {
            const cx = padL + (idx + 0.5) * slotW;
            const count = st.diceCounts[n] ?? 0;
            const exp = st.expectedCounts[n] ?? 0;
            const barH = (count / maxVal) * chartH;
            const expY = baseY - (exp / maxVal) * chartH;
            return {
              n,
              cx,
              bx: cx - barW / 2,
              barW,
              count,
              exp,
              barH,
              barY: baseY - barH,
              expY,
            };
          });
          const expectedPolyline = items.map((d) => `${d.cx.toFixed(1)},${d.expY.toFixed(1)}`).join(' ');
          return { padL, padR, baseY, items, expectedPolyline };
        });

        const vpGeometry = createMemo(() => {
          const st = stats();
          const padL = 30;
          const padR = 14;
          const topY = 14;
          const baseY = 132;
          const plotW = 360 - padL - padR;
          const plotH = baseY - topY;
          const targetVp = props.record.config.victoryPoints;
          if (!st) {
            return {
              padL,
              padR,
              baseY,
              plotW,
              targetVp,
              yTarget: topY,
              midVp: Math.round(targetVp / 2),
              yMid: (topY + baseY) / 2,
              maxTurn: 1,
              series: [],
            };
          }
          const maxVp = Math.max(
            targetVp,
            4,
            ...st.vpTimeline.flatMap((pt) => pt.points),
          );
          const lastTurn = st.vpTimeline[st.vpTimeline.length - 1]?.turn ?? st.totalTurns;
          const maxTurn = Math.max(1, lastTurn);
          const xOf = (turn: number) => padL + (turn / maxTurn) * plotW;
          const yOf = (vp: number) => baseY - (vp / maxVp) * plotH;
          const midVp = Math.round(maxVp / 2);

          const series = Array.from({ length: props.record.config.numPlayers }, (_, seat) => {
            const pts =
              st.vpTimeline.length === 1
                ? [
                    { x: padL, y: yOf(st.vpTimeline[0]!.points[seat] ?? 0) },
                    { x: padL + plotW, y: yOf(st.vpTimeline[0]!.points[seat] ?? 0) },
                  ]
                : st.vpTimeline.map((row) => ({
                    x: xOf(row.turn),
                    y: yOf(row.points[seat] ?? 0),
                  }));
            const lastPt = pts[pts.length - 1] ?? { x: padL, y: baseY };
            const finalScore =
              st.vpTimeline[st.vpTimeline.length - 1]?.points[seat] ?? 0;
            return {
              seat,
              color: PLAYER_CSS[seat] ?? '#eef4f6',
              isWinner: props.record.winner === seat,
              polyline: pts.map((p) => `${p.x.toFixed(1)},${p.y.toFixed(1)}`).join(' '),
              lastX: lastPt.x,
              lastY: lastPt.y,
              finalScore,
            };
          });

          return {
            padL,
            padR,
            baseY,
            plotW,
            targetVp,
            yTarget: yOf(targetVp),
            midVp,
            yMid: yOf(midVp),
            maxTurn,
            series,
          };
        });

        return (
          <section
            class="post-game-stats rules-box"
            data-testid="post-game-stats"
            style={{ gap: '8px', margin: '4px 0' }}
          >
            <div class="tabs seg small" role="tablist" aria-label={t('数据复盘页签', 'Match stats tabs')}>
              <Show when={stats()}>
                <button
                  type="button"
                  role="tab"
                  aria-selected={activeTab() === 'dice'}
                  classList={{ on: activeTab() === 'dice' }}
                  onClick={() => setTab('dice')}
                >
                  {t('骰子分布', 'Dice')}
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={activeTab() === 'vp'}
                  classList={{ on: activeTab() === 'vp' }}
                  onClick={() => setTab('vp')}
                >
                  {t('胜分走势', 'VP Trend')}
                </button>
              </Show>
              <button
                type="button"
                role="tab"
                data-tab="winprob"
                aria-selected={activeTab() === 'winprob'}
                classList={{ on: activeTab() === 'winprob' }}
                onClick={() => setTab('winprob')}
              >
                {t('AI 胜率复盘', 'AI Win-Prob')}
              </button>
              <Show when={stats()}>
                <button
                  type="button"
                  role="tab"
                  aria-selected={activeTab() === 'econ'}
                  classList={{ on: activeTab() === 'econ' }}
                  onClick={() => setTab('econ')}
                >
                  {t('经济明细', 'Economy')}
                </button>
              </Show>
            </div>

            <Show when={activeTab() === 'dice' && stats()}>
              {(s) => (
                <div>
                  <svg
                    viewBox="0 0 360 150"
                    role="img"
                    aria-label={t('骰子分布柱状图', 'Dice distribution bar chart')}
                    style={{ width: '100%', height: 'auto', display: 'block' }}
                  >
                    <line
                      x1={diceGeometry().padL}
                      y1={diceGeometry().baseY}
                      x2={360 - diceGeometry().padR}
                      y2={diceGeometry().baseY}
                      stroke="rgba(255,255,255,0.2)"
                      stroke-width="1"
                    />
                    <line
                      x1="278"
                      y1="10"
                      x2="294"
                      y2="10"
                      stroke="rgba(238,244,246,0.75)"
                      stroke-width="1.3"
                      stroke-dasharray="3 2"
                    />
                    <circle cx="286" cy="10" r="2" fill="#eef4f6" />
                    <text x="298" y="13" font-size="8.5" fill="#9fb7c2">
                      {t('理论期望', 'Expected')}
                    </text>

                    <For each={diceGeometry().items}>
                      {(d) => (
                        <g>
                          <rect
                            x={d.bx}
                            y={d.count > 0 ? Math.min(d.barY, diceGeometry().baseY - 2) : diceGeometry().baseY}
                            width={d.barW}
                            height={d.count > 0 ? Math.max(2, d.barH) : 0}
                            rx="3"
                            fill={diceBarColor(d.n)}
                            opacity="0.9"
                          >
                            <title>
                              {t(
                                `${d.n} 点：实际 ${d.count} 次 / 理论期望 ${d.exp.toFixed(1)} 次`,
                                `Roll ${d.n}: actual ${d.count} / expected ${d.exp.toFixed(1)}`,
                              )}
                            </title>
                          </rect>
                          <text
                            x={d.cx}
                            y={Math.min(d.barY - 4, diceGeometry().baseY - 4)}
                            text-anchor="middle"
                            font-size="9.5"
                            fill="#eef4f6"
                          >
                            {d.count}
                          </text>
                          <text
                            x={d.cx}
                            y="140"
                            text-anchor="middle"
                            font-size="10"
                            font-weight={d.n === 6 || d.n === 8 ? '700' : '400'}
                            fill={d.n === 6 || d.n === 8 ? '#ff9b96' : '#9fb7c2'}
                          >
                            {d.n}
                          </text>
                        </g>
                      )}
                    </For>

                    <polyline
                      fill="none"
                      stroke="rgba(238,244,246,0.78)"
                      stroke-width="1.4"
                      stroke-dasharray="3 2.5"
                      points={diceGeometry().expectedPolyline}
                    />
                    <For each={diceGeometry().items}>
                      {(d) => (
                        <circle cx={d.cx} cy={d.expY} r="2.1" fill="#eef4f6" opacity="0.88">
                          <title>
                            {t(
                              `${d.n} 点理论期望：${d.exp.toFixed(2)} 次`,
                              `Expected rolls for ${d.n}: ${d.exp.toFixed(2)}`,
                            )}
                          </title>
                        </circle>
                      )}
                    </For>
                  </svg>
                  <p class="muted small" style={{ 'margin-top': '4px', 'text-align': 'center' }}>
                    {t(
                      `共掷骰 ${s().totalRolls} 次 · 7点出现 ${s().sevenCount} 次（理论 ${(s().expectedCounts[7] ?? 0).toFixed(1)}）· 最热数字 ${s().hottestRoll ?? '—'}`,
                      `Total rolls ${s().totalRolls} · 7s rolled ${s().sevenCount} (exp ${(s().expectedCounts[7] ?? 0).toFixed(1)}) · Hottest ${s().hottestRoll ?? '—'}`,
                    )}
                  </p>
                </div>
              )}
            </Show>

            <Show when={activeTab() === 'vp' && stats()}>
              <div>
                <svg
                  viewBox="0 0 360 155"
                  role="img"
                  aria-label={t('胜分走势折线图', 'Victory point progression chart')}
                  style={{ width: '100%', height: 'auto', display: 'block' }}
                >
                  <line
                    x1={vpGeometry().padL}
                    y1={vpGeometry().baseY}
                    x2={360 - vpGeometry().padR}
                    y2={vpGeometry().baseY}
                    stroke="rgba(255,255,255,0.2)"
                    stroke-width="1"
                  />
                  <line
                    x1={vpGeometry().padL}
                    y1={vpGeometry().yMid}
                    x2={360 - vpGeometry().padR}
                    y2={vpGeometry().yMid}
                    stroke="rgba(255,255,255,0.07)"
                    stroke-width="1"
                  />
                  <line
                    x1={vpGeometry().padL}
                    y1={vpGeometry().yTarget}
                    x2={360 - vpGeometry().padR}
                    y2={vpGeometry().yTarget}
                    stroke="#f0b429"
                    stroke-width="1.2"
                    stroke-dasharray="4 3"
                    opacity="0.85"
                  />
                  <text
                    x={vpGeometry().padL - 5}
                    y={vpGeometry().yTarget + 3}
                    text-anchor="end"
                    font-size="9"
                    fill="#f0b429"
                  >
                    {t(`${vpGeometry().targetVp}分`, `${vpGeometry().targetVp}VP`)}
                  </text>
                  <text
                    x={vpGeometry().padL - 5}
                    y={vpGeometry().yMid + 3}
                    text-anchor="end"
                    font-size="8.5"
                    fill="#9fb7c2"
                  >
                    {vpGeometry().midVp}
                  </text>
                  <text
                    x={vpGeometry().padL - 5}
                    y={vpGeometry().baseY + 3}
                    text-anchor="end"
                    font-size="8.5"
                    fill="#9fb7c2"
                  >
                    0
                  </text>

                  <text x={vpGeometry().padL} y="148" text-anchor="start" font-size="9" fill="#9fb7c2">
                    T0
                  </text>
                  <text
                    x={vpGeometry().padL + vpGeometry().plotW / 2}
                    y="148"
                    text-anchor="middle"
                    font-size="9"
                    fill="#9fb7c2"
                  >
                    {`T${Math.round(vpGeometry().maxTurn / 2)}`}
                  </text>
                  <text x={360 - vpGeometry().padR} y="148" text-anchor="end" font-size="9" fill="#9fb7c2">
                    {`T${vpGeometry().maxTurn}`}
                  </text>

                  <For each={vpGeometry().series}>
                    {(line) => (
                      <g>
                        <polyline
                          fill="none"
                          stroke={line.color}
                          stroke-width={line.isWinner ? '2.4' : '1.7'}
                          stroke-linejoin="round"
                          stroke-linecap="round"
                          points={line.polyline}
                        />
                        <circle
                          cx={line.lastX}
                          cy={line.lastY}
                          r={line.isWinner ? '3.2' : '2.3'}
                          fill={line.color}
                        >
                          <title>{t(`${props.name(line.seat)}：${line.finalScore} 分`, `${props.name(line.seat)}: ${line.finalScore} VP`)}</title>
                        </circle>
                      </g>
                    )}
                  </For>
                </svg>

                <div
                  class="row small"
                  style={{ gap: '6px 12px', 'justify-content': 'center', 'margin-top': '4px' }}
                >
                  <For each={vpGeometry().series}>
                    {(line) => (
                      <span style={{ display: 'inline-flex', 'align-items': 'center', gap: '4px' }}>
                        <SeatDot seat={line.seat} />
                        <span>{props.name(line.seat)}</span>
                        <span class="muted">{t(`${line.finalScore}分`, `${line.finalScore} VP`)}</span>
                      </span>
                    )}
                  </For>
                </div>
              </div>
            </Show>

            <Show when={activeTab() === 'winprob'}>
              <div>
                <svg
                  viewBox="0 0 360 160"
                  role="img"
                  aria-label={t('多玩家胜率走势图', 'Multi-player win-probability chart')}
                  style={{ width: '100%', height: 'auto', display: 'block' }}
                >
                  {/* 100%, 50%, 0% horizontal gridlines */}
                  <line
                    x1={winProbGeometry().padL}
                    y1={winProbGeometry().topY}
                    x2={360 - winProbGeometry().padR}
                    y2={winProbGeometry().topY}
                    stroke="rgba(240,180,41,0.35)"
                    stroke-width="1"
                    stroke-dasharray="4 3"
                  />
                  <line
                    x1={winProbGeometry().padL}
                    y1={winProbGeometry().midY}
                    x2={360 - winProbGeometry().padR}
                    y2={winProbGeometry().midY}
                    stroke="rgba(255,255,255,0.14)"
                    stroke-width="1"
                    stroke-dasharray="3 3"
                  />
                  <line
                    x1={winProbGeometry().padL}
                    y1={winProbGeometry().baseY}
                    x2={360 - winProbGeometry().padR}
                    y2={winProbGeometry().baseY}
                    stroke="rgba(255,255,255,0.22)"
                    stroke-width="1"
                  />

                  <text
                    x={winProbGeometry().padL - 5}
                    y={winProbGeometry().topY + 3}
                    text-anchor="end"
                    font-size="8.5"
                    fill="#f0b429"
                  >
                    100%
                  </text>
                  <text
                    x={winProbGeometry().padL - 5}
                    y={winProbGeometry().midY + 3}
                    text-anchor="end"
                    font-size="8.5"
                    fill="#9fb7c2"
                  >
                    50%
                  </text>
                  <text
                    x={winProbGeometry().padL - 5}
                    y={winProbGeometry().baseY + 3}
                    text-anchor="end"
                    font-size="8.5"
                    fill="#9fb7c2"
                  >
                    0%
                  </text>

                  {/* X-axis turn ticks */}
                  <text x={winProbGeometry().padL} y="152" text-anchor="start" font-size="9" fill="#9fb7c2">
                    T0
                  </text>
                  <text
                    x={winProbGeometry().padL + winProbGeometry().plotW / 2}
                    y="152"
                    text-anchor="middle"
                    font-size="9"
                    fill="#9fb7c2"
                  >
                    {`T${Math.round(winProbGeometry().maxTurn / 2)}`}
                  </text>
                  <text x={360 - winProbGeometry().padR} y="152" text-anchor="end" font-size="9" fill="#9fb7c2">
                    {`T${winProbGeometry().maxTurn}`}
                  </text>

                  {/* Vertical guide lines for Top 3 turning points */}
                  <For each={winProbGeometry().callouts}>
                    {(c) => (
                      <line
                        x1={c.cx}
                        y1={winProbGeometry().topY}
                        x2={c.cx}
                        y2={winProbGeometry().baseY}
                        stroke="rgba(240,180,41,0.32)"
                        stroke-width="1"
                        stroke-dasharray="2 2.5"
                      />
                    )}
                  </For>

                  {/* Smooth win-probability polyline per player seat */}
                  <For each={winProbGeometry().series}>
                    {(line) => (
                      <g>
                        <polyline
                          fill="none"
                          stroke={line.color}
                          stroke-width={line.isWinner ? '2.6' : '1.65'}
                          stroke-linejoin="round"
                          stroke-linecap="round"
                          opacity={line.isWinner ? '1' : '0.86'}
                          points={line.polyline}
                        />
                        <circle
                          cx={line.lastX}
                          cy={line.lastY}
                          r={line.isWinner ? '3.3' : '2.3'}
                          fill={line.color}
                        >
                          <title>{`${props.name(line.seat)}: ${line.finalPct.toFixed(0)}%`}</title>
                        </circle>
                      </g>
                    )}
                  </For>

                  {/* Numbered golden callout badges (①, ②, ③) */}
                  <For each={winProbGeometry().callouts}>
                    {(c) => (
                      <g
                        style={{ cursor: props.onSeekReplay ? 'pointer' : 'default' }}
                        onClick={() =>
                          props.onSeekReplay?.(props.record, Math.max(0, c.pt.actionIndex - 1))
                        }
                      >
                        <circle
                          cx={c.cx}
                          cy={c.anchorY}
                          r="2.8"
                          fill={c.seatColor}
                          stroke="#0b2230"
                          stroke-width="1"
                        />
                        <circle
                          cx={c.cx}
                          cy={c.badgeY}
                          r="8"
                          fill="#f0b429"
                          stroke="#0b2230"
                          stroke-width="1.3"
                        />
                        <text
                          x={c.cx}
                          y={c.badgeY + 3.6}
                          text-anchor="middle"
                          font-size="10.5"
                          font-weight="700"
                          fill="#1d1a10"
                        >
                          {c.glyph}
                        </text>
                        <title>
                          {`${c.glyph} ${props.name(c.pt.player)} (${t(`第 ${c.pt.turn} 回合`, `Turn ${c.pt.turn}`)}): ${formatWinProbJump(c.pt)}`}
                        </title>
                      </g>
                    )}
                  </For>
                </svg>

                <div
                  class="row small"
                  style={{ gap: '6px 12px', 'justify-content': 'center', 'margin-top': '4px' }}
                >
                  <For each={winProbGeometry().series}>
                    {(line) => (
                      <span style={{ display: 'inline-flex', 'align-items': 'center', gap: '4px' }}>
                        <SeatDot seat={line.seat} />
                        <span>{props.name(line.seat)}</span>
                        <span class="muted">{`${line.finalPct.toFixed(0)}%`}</span>
                      </span>
                    )}
                  </For>
                </div>

                {/* Key Turning Points Coach */}
                <div style={{ 'margin-top': '8px', display: 'flex', 'flex-direction': 'column', gap: '6px' }}>
                  <div
                    style={{
                      display: 'flex',
                      'align-items': 'center',
                      'justify-content': 'space-between',
                      gap: '8px',
                    }}
                  >
                    <span style={{ 'font-size': '12.5px', 'font-weight': '600', color: '#f0b429' }}>
                      {t('Top 3 胜负手智能点评', 'Top 3 Key Turning Points Coach')}
                    </span>
                    <span class="muted small">
                      {t('按单步胜率波动排序', 'Ranked by win-rate swing')}
                    </span>
                  </div>

                  <Show
                    when={winProbGeometry().topTurningPoints.length > 0}
                    fallback={
                      <p class="muted small" style={{ 'text-align': 'center', padding: '6px 0' }}>
                        {t(
                          '本局节奏平稳，未检测到大幅胜率突变节点。',
                          'Steady match pace — no major win-probability swings detected.',
                        )}
                      </p>
                    }
                  >
                    <For each={winProbGeometry().topTurningPoints}>
                      {(pt, idx) => {
                        const glyph = () => CALLOUT_GLYPHS[idx()] ?? '①';
                        const isPositive = () => pt.deltaPct >= 0;
                        const coach = createMemo(() => describeTurningPoint(pt));
                        return (
                          <div
                            data-testid={`turning-point-${idx() + 1}`}
                            style={{
                              display: 'flex',
                              'flex-direction': 'column',
                              gap: '4px',
                              padding: '7px 9px',
                              'border-radius': '8px',
                              background: 'rgba(255, 255, 255, 0.045)',
                              border: '1px solid rgba(255, 255, 255, 0.09)',
                              'border-left': `3px solid ${isPositive() ? '#46a758' : '#e5484d'}`,
                            }}
                          >
                            <div
                              style={{
                                display: 'flex',
                                'flex-wrap': 'wrap',
                                'align-items': 'center',
                                gap: '6px',
                              }}
                            >
                              <span
                                style={{
                                  color: '#f0b429',
                                  'font-weight': '700',
                                  'font-size': '14px',
                                  'line-height': '1',
                                }}
                              >
                                {glyph()}
                              </span>
                              <SeatDot seat={pt.player} />
                              <b style={{ 'font-size': '12.5px' }}>{props.name(pt.player)}</b>
                              <span class="muted small">
                                {t(`第 ${pt.turn} 回合`, `Turn ${pt.turn}`)}
                              </span>
                              <span class="badge" style={{ 'font-size': '10.5px', padding: '0 6px' }}>
                                {coach().tag}
                              </span>
                              <span
                                style={{
                                  'margin-left': 'auto',
                                  'font-size': '11.5px',
                                  'font-weight': '600',
                                  'font-variant-numeric': 'tabular-nums',
                                  padding: '1px 7px',
                                  'border-radius': '999px',
                                  background: isPositive()
                                    ? 'rgba(70, 167, 88, 0.22)'
                                    : 'rgba(229, 72, 77, 0.22)',
                                  border: isPositive()
                                    ? '1px solid rgba(95, 207, 128, 0.45)'
                                    : '1px solid rgba(229, 72, 77, 0.5)',
                                  color: isPositive() ? '#7ee29a' : '#ff9b9e',
                                }}
                              >
                                {formatWinProbJump(pt)}
                              </span>
                            </div>

                            <p style={{ margin: '0', 'font-size': '12.5px', 'line-height': '1.42' }}>
                              {coach().headline}
                            </p>

                            <div
                              style={{
                                display: 'flex',
                                'flex-wrap': 'wrap',
                                'align-items': 'center',
                                'justify-content': 'space-between',
                                gap: '6px',
                              }}
                            >
                              <span
                                class="muted small"
                                style={{ flex: '1 1 190px', 'line-height': '1.35' }}
                              >
                                {coach().takeaway}
                              </span>
                              <Show when={props.onSeekReplay}>
                                <button
                                  type="button"
                                  class="small"
                                  style={{
                                    padding: '2px 9px',
                                    'font-size': '11.5px',
                                    'border-color': 'rgba(240, 180, 41, 0.45)',
                                    color: '#f0b429',
                                    background: 'rgba(240, 180, 41, 0.12)',
                                    'white-space': 'nowrap',
                                  }}
                                  onClick={() =>
                                    props.onSeekReplay?.(props.record, Math.max(0, pt.actionIndex - 1))
                                  }
                                >
                                  {t('跳转至此步', 'Watch Move')}
                                </button>
                              </Show>
                            </div>
                          </div>
                        );
                      }}
                    </For>
                  </Show>
                </div>
              </div>
            </Show>

            <Show when={activeTab() === 'econ' && stats()}>
              {(s) => (
                <div style={{ 'overflow-x': 'auto' }}>
                  <table
                    class="stats-table"
                    style={{
                      width: '100%',
                      'border-collapse': 'collapse',
                      'font-size': '12.5px',
                      'font-variant-numeric': 'tabular-nums',
                    }}
                  >
                    <thead>
                      <tr class="muted">
                        <th style={{ 'text-align': 'left', 'font-weight': '500', padding: '4px 6px' }}>
                          {t('玩家', 'Player')}
                        </th>
                        <th style={{ 'text-align': 'right', 'font-weight': '500', padding: '4px 6px' }}>
                          {t('产出', 'Yield')}
                        </th>
                        <th style={{ 'text-align': 'right', 'font-weight': '500', padding: '4px 6px' }}>
                          {t('交易', 'Trades')}
                        </th>
                        <th style={{ 'text-align': 'right', 'font-weight': '500', padding: '4px 6px' }}>
                          {t('掠夺/被抢', 'Rob +/−')}
                        </th>
                        <th style={{ 'text-align': 'right', 'font-weight': '500', padding: '4px 6px' }}>
                          {t('爆牌弃置', 'Discarded')}
                        </th>
                      </tr>
                    </thead>
                    <tbody>
                      <For each={s().economy}>
                        {(econ) => {
                          const baseProduced = () => econ.produced + econ.gold;
                          const totalTrades = () => econ.maritimeTrades + econ.playerTrades;
                          return (
                            <tr style={{ 'border-top': '1px solid rgba(255,255,255,0.06)' }}>
                              <td style={{ padding: '5px 6px', display: 'flex', 'align-items': 'center', gap: '6px' }}>
                                <SeatDot seat={econ.seat} />
                                <span>{props.name(econ.seat)}</span>
                                <Show when={econ.barbarianPoints > 0}>
                                  <span class="badge" title={t('击退黑帆独得卫城分', 'Defender of Catan VP')}>
                                    {t(`+${econ.barbarianPoints}卫`, `+${econ.barbarianPoints}Def`)}
                                  </span>
                                </Show>
                              </td>
                              <td
                                style={{ padding: '5px 6px', 'text-align': 'right' }}
                                title={
                                  econ.gold > 0
                                    ? t(
                                        `常规产出 ${econ.produced} + 金矿/水渠自选 ${econ.gold}`,
                                        `Standard ${econ.produced} + Gold/Aqueduct ${econ.gold}`,
                                      )
                                    : t(`常规产出 ${econ.produced}`, `Standard ${econ.produced}`)
                                }
                              >
                                {isCitadels()
                                  ? t(`${baseProduced()}+${econ.commodities}货`, `${baseProduced()}+${econ.commodities}C`)
                                  : baseProduced()}
                              </td>
                              <td
                                style={{ padding: '5px 6px', 'text-align': 'right' }}
                                title={t(
                                  `海贸 ${econ.maritimeTrades} 次 / 玩家交易 ${econ.playerTrades} 次`,
                                  `Maritime ${econ.maritimeTrades} / Player ${econ.playerTrades}`,
                                )}
                              >
                                {totalTrades()}
                              </td>
                              <td style={{ padding: '5px 6px', 'text-align': 'right' }}>
                                +{econ.stolenGain} / -{econ.stolenLoss}
                              </td>
                              <td style={{ padding: '5px 6px', 'text-align': 'right' }}>{econ.discarded}</td>
                            </tr>
                          );
                        }}
                      </For>
                    </tbody>
                  </table>
                </div>
              )}
            </Show>
          </section>
        );
      }}
    </Show>
  );
}
