// Labels and event descriptions (zh-CN).
import type { DevCard, Event, Resource, ResourceSet, RuleError } from '../generated/proto';

export const RESOURCES: Resource[] = ['Brick', 'Lumber', 'Wool', 'Grain', 'Ore'];
export const RESOURCE_NAME: Record<Resource, string> = { Brick: '砖', Lumber: '木', Wool: '羊毛', Grain: '小麦', Ore: '矿石' };
export const RESOURCE_SHORT: Record<Resource, string> = { Brick: '砖', Lumber: '木', Wool: '羊', Grain: '麦', Ore: '矿' };
export const RESOURCE_CSS = ['#c0623a', '#2f7d4a', '#9fd27a', '#e2b74a', '#8a8f99'] as const;

export const DEV_CARDS: DevCard[] = ['Knight', 'VictoryPoint', 'RoadBuilding', 'YearOfPlenty', 'Monopoly'];
export const DEV_NAME: Record<DevCard, string> = {
  Knight: '骑士',
  VictoryPoint: '胜利点',
  RoadBuilding: '道路建设',
  YearOfPlenty: '丰收',
  Monopoly: '垄断',
};
export const DEV_HELP: Record<DevCard, string> = {
  Knight: '移动强盗并抢一张牌，计入最大骑士',
  VictoryPoint: '1 分（对他人隐藏）',
  RoadBuilding: '免费建 2 条道路',
  YearOfPlenty: '从银行拿 2 张任选资源',
  Monopoly: '所有对手交出指定资源',
};

export const COSTS: { label: string; cost: ResourceSet; points: string }[] = [
  { label: '道路', cost: [1, 1, 0, 0, 0], points: '—' },
  { label: '村庄', cost: [1, 1, 1, 1, 0], points: '1 分' },
  { label: '城市', cost: [0, 0, 0, 2, 3], points: '2 分' },
  { label: '发展卡', cost: [0, 0, 1, 1, 1], points: '?' },
];

const ERRORS: Record<RuleError, string> = {
  GameOver: '游戏已经结束',
  InvalidPlayer: '没有这名玩家',
  NotYourTurn: '还没轮到你',
  WrongPhase: '现在不能这样做',
  InsufficientResources: '资源不足',
  NoPiecesLeft: '这种棋子已经用完了',
  InvalidVertex: '无效的位置',
  InvalidEdge: '无效的位置',
  Occupied: '这里已经有人了',
  TooCloseToBuilding: '离其他建筑太近',
  NotConnected: '没有连到你的道路',
  NotYourSettlement: '这里不是你的村庄',
  InvalidTile: '无效的地块',
  RobberMustMove: '强盗必须移到别的地块',
  InvalidVictim: '不能抢这名玩家',
  NoSuchCard: '没有可以打出的这种卡',
  DevCardAlreadyPlayed: '每回合只能打一张发展卡',
  NoRoadSpot: '没有可以修路的位置',
  DeckEmpty: '发展卡已经卖完了',
  BankShort: '银行没有这些资源',
  InvalidTrade: '无效的交易',
  NoOpenTrade: '当前没有交易报价',
  PartnerHasNotAccepted: '对方还没有接受',
  WrongDiscardAmount: '必须正好弃掉一半手牌',
  NothingToDiscard: '你不需要弃牌',
};

const SERVER_ERRORS: Record<string, string> = {
  Stale: '状态已更新，请重试',
  NoSuchAction: '现在不能这样做',
  Offline: '连接已断开，正在重连…',
  BadMessage: '请求无效',
  RoomNotFound: '房间不存在或已关闭',
  TooManyRooms: '服务器房间已满，请稍后再试',
  NotHost: '只有房主可以这样做',
  AlreadyStarted: '游戏已经开始',
  NotStarted: '游戏还没开始',
  Spectator: '观众不能操作',
  RateLimited: '发言太快了',
  InvalidSeat: '不能修改这个座位',
};

export function errorText(code: string): string {
  return ERRORS[code as RuleError] ?? SERVER_ERRORS[code] ?? code;
}

export function setText(set: ResourceSet): string {
  const parts = RESOURCES.flatMap((r, i) => (set[i] > 0 ? [`${set[i]} ${RESOURCE_SHORT[r]}`] : []));
  return parts.length ? parts.join('、') : '无';
}

export const setTotal = (set: ResourceSet) => set.reduce((a, b) => a + b, 0);

/** Text for one event, or null for events that are not worth a log line. */
export function describe(e: Event, name: (seat: number) => string): string | null {
  switch (e.type) {
    case 'DiceRolled': {
      const [a, b] = e.value.dice;
      return `${name(e.value.player)} 掷出 ${a} + ${b} = ${a + b}`;
    }
    case 'ResourcesProduced':
      return `${name(e.value.player)} 获得 ${setText(e.value.resources)}`;
    case 'StartingResources':
      return `${name(e.value.player)} 获得起始资源 ${setText(e.value.resources)}`;
    case 'Discarded':
      return `${name(e.value.player)} 弃掉 ${setText(e.value.resources)}`;
    case 'RobberMoved':
      return `${name(e.value.player)} 移动了强盗`;
    case 'Stolen': {
      const what = e.value.resource ? `1 张${RESOURCE_NAME[e.value.resource]}` : '1 张牌';
      return `${name(e.value.thief)} 从 ${name(e.value.victim)} 手里抢走 ${what}`;
    }
    case 'RoadBuilt':
      return `${name(e.value.player)} 修了一条道路`;
    case 'SettlementBuilt':
      return `${name(e.value.player)} 建立了村庄`;
    case 'CityBuilt':
      return `${name(e.value.player)} 把村庄升级为城市`;
    case 'DevCardBought':
      return `${name(e.value.player)} 购买了发展卡${e.value.card ? `（${DEV_NAME[e.value.card]}）` : ''}`;
    case 'DevCardPlayed':
      return `${name(e.value.player)} 打出【${DEV_NAME[e.value.card]}】`;
    case 'YearOfPlentyTaken':
      return `${name(e.value.player)} 从银行拿取 ${setText(e.value.resources)}`;
    case 'MonopolyTaken': {
      const total = e.value.taken.reduce((a, b) => a + b, 0);
      return `${name(e.value.player)} 垄断了${RESOURCE_NAME[e.value.resource]}，共收走 ${total} 张`;
    }
    case 'MaritimeTraded':
      return `${name(e.value.player)} 与银行交易：${setText(e.value.give)} → 1 ${RESOURCE_SHORT[e.value.get]}`;
    case 'TradeProposed':
      return `${name(e.value.player)} 发起交易：给 ${setText(e.value.give)}，换 ${setText(e.value.get)}`;
    case 'TradeResponded':
      return `${name(e.value.player)} ${e.value.accept ? '接受' : '拒绝'}了交易`;
    case 'TradeExecuted':
      return `${name(e.value.player)} 与 ${name(e.value.partner)} 完成交易`;
    case 'TradeCancelled':
      return null;
    case 'LongestRoadChanged':
      return e.value.owner === null ? '最长道路暂时无人持有' : `${name(e.value.owner)} 获得【最长道路】`;
    case 'LargestArmyChanged':
      return e.value.owner === null ? '最大骑士暂时无人持有' : `${name(e.value.owner)} 获得【最大骑士】`;
    case 'TurnEnded':
      return null;
    case 'GameWon':
      return `${name(e.value.player)} 以 ${e.value.points} 分获胜！`;
  }
}
