// Labels and event descriptions (zh-CN).
import type {
  BotLevel,
  CityTrack,
  Commodity,
  CommoditySet,
  DevCard,
  Event,
  Resource,
  ResourceSet,
  RuleError,
  Terrain,
  Tier,
  TradeGood,
} from '../generated/proto';

export const BOT_LEVEL_NAME: Record<BotLevel, string> = { Easy: '简单', Normal: '普通', Hard: '困难' };

export const TIER_NAME: Record<Tier, string> = { Bronze: '青铜', Silver: '白银', Gold: '黄金', Platinum: '铂金', Diamond: '钻石', Master: '大师' };

export const RESOURCES: Resource[] = ['Brick', 'Lumber', 'Wool', 'Grain', 'Ore'];
export const RESOURCE_NAME: Record<Resource, string> = { Brick: '砖', Lumber: '木', Wool: '羊毛', Grain: '小麦', Ore: '矿石' };
export const RESOURCE_SHORT: Record<Resource, string> = { Brick: '砖', Lumber: '木', Wool: '羊', Grain: '麦', Ore: '矿' };
export const RESOURCE_CSS = ['#c0623a', '#2f7d4a', '#9fd27a', '#e2b74a', '#8a8f99'] as const;

export const COMMODITIES: Commodity[] = ['Paper', 'Cloth', 'Coin'];
export const COMMODITY_NAME: Record<Commodity, string> = { Paper: '造纸', Cloth: '织布', Coin: '铸币' };
export const COMMODITY_SHORT: Record<Commodity, string> = { Paper: '纸', Cloth: '布', Coin: '币' };
export const COMMODITY_CSS = ['#b89b67', '#9b59d0', '#d99e1a'] as const;

export const CITY_TRACKS: CityTrack[] = ['Trade', 'Politics', 'Science'];
export const TRACK_NAME: Record<CityTrack, string> = { Trade: '商贸', Politics: '军政', Science: '学术' };
export const TRACK_COMMODITY: Record<CityTrack, Commodity> = { Trade: 'Cloth', Politics: 'Coin', Science: 'Paper' };
export const TRACK_PERK: Record<CityTrack, string> = {
  Trade: 'Lv3 商馆：任意商品 2:1 交易',
  Politics: 'Lv3 帅印：铁卫可晋升至 3 阶',
  Science: 'Lv3 水渠：掷骰轮空任选 1 资源',
};

export const TERRAIN_NAME: Record<Terrain, string> = {
  Hills: '丘陵',
  Forest: '森林',
  Pasture: '牧场',
  Fields: '农田',
  Mountains: '山脉',
  Desert: '沙漠',
  Sea: '海面',
  Gold: '金矿',
  Fog: '迷雾',
};

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
  RoadBuilding: '免费建 2 条道路（航海地图上也可以是船）',
  YearOfPlenty: '从银行拿 2 张任选资源',
  Monopoly: '所有对手交出指定资源',
};

export const COSTS: { label: string; cost: ResourceSet; points: string }[] = [
  { label: '道路', cost: [1, 1, 0, 0, 0], points: '—' },
  { label: '村庄', cost: [1, 1, 1, 1, 0], points: '1 分' },
  { label: '城市', cost: [0, 0, 0, 2, 3], points: '2 分' },
  { label: '发展卡', cost: [0, 0, 1, 1, 1], points: '?' },
  { label: '船', cost: [0, 1, 1, 0, 0], points: '—' },
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
  WrongGround: '道路修在陆地边上，船造在临海的边上',
  PirateBlocks: '海盗挡住了那里',
  ShipAlreadyMoved: '每回合只能移动一艘船',
  ShipCannotMove: '这艘船不能移动',
  NoGoldOwed: '你没有要选的金矿资源',
  WrongGoldAmount: '要选够金矿给你的数量',
  FriendlyRobber: '友好强盗：不能针对公开分 ≤ 2 分的玩家',
  NoCityForUpgrade: '需要先拥有城市才能升级城邦科技',
  InvalidMetropolis: '请选择一座尚未加冕的普通城市作为大都会',
  NotYourKnight: '这里不是你的铁卫',
  KnightNotActive: '这名铁卫尚未激活',
  KnightCannotAct: '这名铁卫本回合不能执行此操作',
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
  Kicked: '你已被房主移出座位，现为观众状态',
  GameNotOver: '对局尚未结束',
  BadCustomCode: '自定义地图短码格式无效',
  MissingDesert: '自定义地图至少需要 1 格沙漠',
  MissingSea: '航海地图至少需要 1 格海面',
  UnexpectedSea: '当前剧本不支持海面地块',
  UnexpectedFog: '当前剧本不支持迷雾地块',
  TooMuchFog: '迷雾地块数量超出上限',
  FogTouchesLand: '迷雾地块需由海面与初始陆地隔开',
  TooLittleLand: '初始可建村陆地交叉点不足',
  BadReplay: '不是有效的回放文件',
  UnsupportedReplay: '不支持这个版本的回放文件',
  BadReplaySeed: '回放文件已损坏',
  BadReplayConfig: '回放文件已损坏',
  ReplayNotFound: '回放不存在或已过期',
  // Accounts
  MissingCredentials: '请输入用户名和密码',
  InvalidUsername: '用户名只能用字母、数字、_ 和 -，最长 32 位',
  PasswordTooShort: '密码至少需要 12 个字符',
  PasswordTooLong: '密码太长了',
  UsernameTaken: '这个用户名已经有人用了',
  InvalidCredentials: '用户名或密码不对',
  TooManyAttempts: '尝试次数太多，请稍后再试',
  Busy: '服务器繁忙，请稍后再试',
  AccountsDisabled: '这个服务器没有开启账号功能',
  SessionExpired: '登录已失效，请重新登录后再加入',
  NotLoggedIn: '登录已失效，请重新登录',
  BadRequest: '请求无效',
  Internal: '服务器出错了，请稍后再试',
  ServerError: '服务器出错了，正在重试…',
  NetworkError: '网络连接失败',
  PlayerNotFound: '找不到这名玩家',
};

export function errorText(code: string): string {
  if (code.startsWith('BadReplayAction')) return '回放文件已损坏（动作无法重放）';
  return ERRORS[code as RuleError] ?? SERVER_ERRORS[code] ?? code;
}

export function setText(set: ResourceSet): string {
  const parts = RESOURCES.flatMap((r, i) => (set[i] > 0 ? [`${set[i]} ${RESOURCE_SHORT[r]}`] : []));
  return parts.length ? parts.join('、') : '无';
}

export function commSetText(set: CommoditySet): string {
  const parts = COMMODITIES.flatMap((c, i) => (set[i] > 0 ? [`${set[i]} ${COMMODITY_SHORT[c]}`] : []));
  return parts.length ? parts.join('、') : '无';
}

export function goodName(g: TradeGood): string {
  return g.type === 'Resource' ? RESOURCE_SHORT[g.value] : COMMODITY_SHORT[g.value];
}

export const setTotal = (set: readonly number[]) => set.reduce((a, b) => a + b, 0);

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
    case 'ShipBuilt':
      return `${name(e.value.player)} 造了一艘船`;
    case 'ShipMoved':
      return `${name(e.value.player)} 移动了一艘船`;
    case 'PirateMoved':
      return `${name(e.value.player)} 移动了海盗`;
    case 'GoldChosen':
      return `${name(e.value.player)} 从金矿选取 ${setText(e.value.resources)}`;
    case 'IslandSettled':
      return `${name(e.value.player)} 登上新的岛屿，奖励 ${e.value.points} 分`;
    case 'TileRevealed': {
      const what = TERRAIN_NAME[e.value.terrain];
      const num = e.value.number ? `（${e.value.number}）` : '';
      const got = e.value.reward ? `，奖励 1 ${RESOURCE_SHORT[e.value.reward]}` : '';
      return `${name(e.value.player)} 拨开迷雾，发现${what}${num}${got}`;
    }
    case 'CommoditiesProduced':
      return `${name(e.value.player)} 获得商品 ${commSetText(e.value.commodities)}`;
    case 'CommoditiesDiscarded':
      return `${name(e.value.player)} 弃掉商品 ${commSetText(e.value.commodities)}`;
    case 'CommodityStolen': {
      const what = e.value.commodity ? `1 张${COMMODITY_NAME[e.value.commodity]}` : '1 张牌';
      return `${name(e.value.thief)} 从 ${name(e.value.victim)} 手里抢走 ${what}`;
    }
    case 'GoodTraded':
      return `${name(e.value.player)} 与银行交易：${e.value.ratio} ${goodName(e.value.give)} → 1 ${goodName(e.value.get)}`;
    case 'CityUpgraded': {
      const metro = e.value.metropolis !== null ? `，加冕【${TRACK_NAME[e.value.track]}大都会】！` : '';
      return `${name(e.value.player)} 将【${TRACK_NAME[e.value.track]}】升至 Lv.${e.value.level}${metro}`;
    }
    case 'KnightRecruited':
      return `${name(e.value.player)} 招募了一名初阶铁卫`;
    case 'KnightActivated':
      return `${name(e.value.player)} 激活了铁卫`;
    case 'KnightPromoted':
      return `${name(e.value.player)} 将铁卫晋升至 ${e.value.level} 阶`;
    case 'KnightMoved': {
      const disp = e.value.displaced
        ? `，驱逐了 ${name(e.value.displaced[0])} 的铁卫${e.value.displaced[1] === null ? '（撤回库存）' : ''}`
        : '';
      return `${name(e.value.player)} 调遣了铁卫${disp}`;
    }
    case 'BarbarianAdvanced':
      return e.value.pos === 6 ? '黑帆舰队逼近（6/7），下一次前进即将攻城！' : null;
    case 'BarbariansRepelled':
      return e.value.sole_defender !== null
        ? `击退黑帆舰队（战力 ${e.value.defense}/${e.value.cities}）！${name(e.value.sole_defender)} 独得 1 点卫城勋章`
        : `击退黑帆舰队（战力 ${e.value.defense}/${e.value.cities}），并列首功者各获 1 张发展卡`;
    case 'CityPillaged':
      return `黑帆舰队攻破防线，${name(e.value.player)} 的一座城市沦陷降为村庄！`;
    case 'FishCaught':
      return `${name(e.value.player)} 捕得 ${e.value.count} 条海鱼`;
    case 'OldBootDrawn':
      return `${name(e.value.player)} 捞到了【破旧皮靴】（获胜目标 +1 分）！`;
    case 'OldBootPassed':
      return `${name(e.value.from)} 将【破旧皮靴】转送给 ${name(e.value.to)}！`;
    case 'FishSpent':
      return `${name(e.value.player)} 消耗 ${e.value.cost} 条海鱼兑换奇策`;
    case 'FishResourceTaken':
      return `${name(e.value.player)} 用海鱼从银行选取 1 ${RESOURCE_SHORT[e.value.resource]}`;
    case 'TurnEnded':
    case 'SpecialBuildPassed':
    case 'PairedTurnEnded':
    case 'GoldOwed':
      return null;
    case 'GameWon':
      return `${name(e.value.player)} 以 ${e.value.points} 分获胜！`;
  }
}
