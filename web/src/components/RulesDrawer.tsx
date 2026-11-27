import { createEffect, createMemo, createSignal, For, Show } from 'solid-js';
import type { HouseRules, Scenario } from '../generated/proto';
import { CommIcon, Modal, ResIcon } from './bits';

export type RulesVariantInfo = Partial<HouseRules> & {
  customMap?: string | boolean | null;
  customBoard?: unknown;
  mapCode?: string | null;
};

const SCENARIO_TABS: { id: Scenario; label: string; vp: number; tagline: string; bullets: string[] }[] = [
  {
    id: 'Classic',
    label: '经典',
    vp: 10,
    tagline: '基础六边形岛屿开拓 · 默认 10 分获胜（支持 2–6 人）',
    bullets: [
      '蛇形两轮放置起始村庄+道路，第二轮放下的村庄立即获得相邻地形各 1 张基础资源。',
      '掷出 7 点时手牌超过 7 张的玩家弃一半（向下取整），随后当前玩家移动强盗并盲抽 1 张牌。',
      '海上贸易默认 4:1，通用港口 3:1，专用资源港 2:1；主阶段内交易、建造与出卡顺序不限。',
      '连续 5+ 条路争【最长道路】(+2分)，打出 3+ 张骑士争【最大骑士】(+2分)；5–6 人启用扩展岛屿与副回合机制。',
    ],
  },
  {
    id: 'Archipelago',
    label: '航海 · 群岛',
    vp: 12,
    tagline: '跨海造船与金矿外岛 · 默认 12 分获胜（支持 2–6 人）',
    bullets: [
      '开局两座村庄均建在西侧主岛，沿海边可自由选择修筑陆路或建造帆船出海。',
      '首次在开局主岛以外的每座外岛建立村庄，立即获得 +2 分登岛奖励（公开分）。',
      '外岛金矿掷出对应点数且无强盗时，相邻每座村庄任选 1 张、每座城市任选 2 张银行资源。',
      '掷出 7 或打骑士可改移海盗封锁海域（禁造船/移船）并抢沿岸船主 1 张牌；路与船仅能在自家聚落处换乘。',
    ],
  },
  {
    id: 'FogIslands',
    label: '航海 · 迷雾',
    vp: 12,
    tagline: '未知海域探索翻牌 · 默认 12 分获胜（支持 2–6 人）',
    bullets: [
      '从西侧月牙主岛扬帆驶向未知迷雾，修路、造船或移船只要端点触及迷雾格即立刻翻开地形。',
      '翻出常规产资源陆地立得 1 张对应资源，翻出金矿立即免费任选 1 张银行资源，翻出海面即可继续通航。',
      '首次在迷雾区翻出的新大陆上建立村庄，获得 +2 分新大陆开拓奖励。',
      '强盗与海盗不可移入尚未翻开的迷雾格；翻开后分别遵循常规陆地强盗与海域海盗规则。',
    ],
  },
  {
    id: 'Citadels',
    label: '城邦与铁卫',
    vp: 13,
    tagline: '商品科技、大都会与黑帆入侵 · 默认 13 分获胜（支持 2–6 人）',
    bullets: [
      '蛇形第二轮直接放置起始城市+道路并拿起始资源；森林/牧场/山脉城市产出 1 基础资源 + 1 商品（纸/布/币，计入手牌上限）。',
      '消耗对应商品升级商贸/军政/学术三系科技：Lv3 解锁专属特权，率先达 Lv4 加冕 +2 分大都会（共 4 分且免疫黑帆降级），他人先达 Lv5 可夺走。',
      '在己方道路交叉点招募(1羊1矿)并激活(1麦)棋盘实体铁卫，可阻断敌路、调遣驱逐低阶敌卫或驱赶相邻强盗。',
      '每次掷骰 50% 推进黑帆舰队，满 7 格触发攻城检定：总激活战力 ≥ 普通城市数则独一首功得 +1 卫城分（并列摸发展卡），失守则出力最少者城市降为村庄。',
    ],
  },
];

export function RulesModal(props: {
  scenario: Scenario;
  rules?: RulesVariantInfo | null;
  customMap?: string | boolean | null;
  onClose: () => void;
}) {
  const [selectedScenario, setSelectedScenario] = createSignal<Scenario>(props.scenario);
  createEffect(() => {
    setSelectedScenario(props.scenario);
  });

  const currentScenarioInfo = createMemo(
    () => SCENARIO_TABS.find((s) => s.id === selectedScenario()) ?? SCENARIO_TABS[0],
  );
  const isSeafaringTab = () =>
    selectedScenario() === 'Archipelago' || selectedScenario() === 'FogIslands';
  const isCitadelsTab = () => selectedScenario() === 'Citadels';

  const activeVariants = createMemo(() => {
    const r = props.rules;
    const tags: string[] = [];
    if (r?.victoryPoints !== null && r?.victoryPoints !== undefined && r.victoryPoints > 0) {
      tags.push(`自定义胜利分 ${r.victoryPoints} 分`);
    }
    if (r?.friendlyRobber) {
      tags.push('友好强盗（≤2分免扰）');
    }
    if (r?.pairedTurns) {
      tags.push('5–6人双人搭档回合');
    }
    if (r?.turnTimerSecs && r.turnTimerSecs > 0) {
      tags.push(`每步限时 ${r.turnTimerSecs}秒`);
    }
    if (props.customMap || r?.customMap || r?.customBoard || r?.mapCode) {
      tags.push('DIY 自定义地图');
    }
    return tags;
  });

  return (
    <Modal title="规则速查与建造花费" wide onClose={props.onClose}>
      <Show when={activeVariants().length > 0}>
        <div
          class="rules-active-bar row small"
          data-testid="active-house-rules"
          style={{
            padding: '6px 10px',
            'border-radius': '8px',
            background: 'rgba(240, 180, 41, 0.12)',
            border: '1px solid rgba(240, 180, 41, 0.35)',
            gap: '6px',
          }}
        >
          <b style={{ color: 'var(--accent)' }}>当前房规：</b>
          <For each={activeVariants()}>{(tag) => <span class="badge">{tag}</span>}</For>
        </div>
      </Show>

      <div class="rules-section">
        <h4>四大剧本要点</h4>
        <div class="tabs seg small" role="tablist" aria-label="剧本规则切换" style={{ 'margin-bottom': '8px' }}>
          <For each={SCENARIO_TABS}>
            {(sc) => (
              <button
                type="button"
                role="tab"
                data-scenario={sc.id}
                aria-selected={selectedScenario() === sc.id}
                classList={{ on: selectedScenario() === sc.id }}
                onClick={() => setSelectedScenario(sc.id)}
              >
                {sc.label}
              </button>
            )}
          </For>
        </div>
        <div class="rules-box" style={{ gap: '6px' }}>
          <div class="row small" style={{ 'justify-content': 'space-between' }}>
            <b>{currentScenarioInfo().tagline}</b>
            <Show when={selectedScenario() === props.scenario}>
              <span class="badge">当前所选</span>
            </Show>
          </div>
          <ul
            class="small"
            style={{
              margin: '0',
              'padding-left': '18px',
              display: 'flex',
              'flex-direction': 'column',
              gap: '4px',
              color: 'var(--text)',
            }}
          >
            <For each={currentScenarioInfo().bullets}>{(line) => <li>{line}</li>}</For>
          </ul>
        </div>
      </div>

      <div class="rules-section">
        <h4>建造与行动花费速查表</h4>
        <div class="rules-box" style={{ gap: '6px' }}>
          <div
            class="row small"
            style={{ 'justify-content': 'space-between', 'border-bottom': '1px solid rgba(255,255,255,0.06)', 'padding-bottom': '5px' }}
          >
            <span style={{ display: 'inline-flex', 'align-items': 'center', gap: '5px', 'min-width': '135px' }}>
              <b>道路</b>
              <ResIcon r={0} />
              <ResIcon r={1} />
            </span>
            <span class="muted" style={{ flex: '1', 'text-align': 'right' }}>
              连通陆路，连续 5+ 争最长商路 (+2分)
            </span>
          </div>

          <div
            class="row small"
            style={{
              'justify-content': 'space-between',
              'border-bottom': '1px solid rgba(255,255,255,0.06)',
              padding: isSeafaringTab() ? '4px 6px' : '0 0 5px',
              'border-radius': isSeafaringTab() ? '6px' : '0',
              background: isSeafaringTab() ? 'rgba(42, 129, 153, 0.22)' : 'transparent',
            }}
          >
            <span style={{ display: 'inline-flex', 'align-items': 'center', gap: '5px', 'min-width': '135px' }}>
              <b>帆船</b>
              <ResIcon r={1} />
              <ResIcon r={2} />
            </span>
            <span class="muted" style={{ flex: '1', 'text-align': 'right' }}>
              航海专属：跨海航线，每回合可移船 1 艘（自家聚落处路船换乘）
            </span>
          </div>

          <div
            class="row small"
            style={{ 'justify-content': 'space-between', 'border-bottom': '1px solid rgba(255,255,255,0.06)', 'padding-bottom': '5px' }}
          >
            <span style={{ display: 'inline-flex', 'align-items': 'center', gap: '4px', 'min-width': '135px' }}>
              <b>村庄</b>
              <ResIcon r={0} />
              <ResIcon r={1} />
              <ResIcon r={2} />
              <ResIcon r={3} />
            </span>
            <span class="muted" style={{ flex: '1', 'text-align': 'right' }}>
              +1 分，需遵守隔一格距离规则
            </span>
          </div>

          <div
            class="row small"
            style={{ 'justify-content': 'space-between', 'border-bottom': '1px solid rgba(255,255,255,0.06)', 'padding-bottom': '5px' }}
          >
            <span style={{ display: 'inline-flex', 'align-items': 'center', gap: '4px', 'min-width': '135px' }}>
              <b>城市</b>
              <span>2×</span>
              <ResIcon r={3} />
              <span>3×</span>
              <ResIcon r={4} />
            </span>
            <span class="muted" style={{ flex: '1', 'text-align': 'right' }}>
              村庄升级（共 2 分），双倍产出；城邦林/牧/山产 1 资源 + 1 商品
            </span>
          </div>

          <div class="row small" style={{ 'justify-content': 'space-between' }}>
            <span style={{ display: 'inline-flex', 'align-items': 'center', gap: '4px', 'min-width': '135px' }}>
              <b>发展卡</b>
              <ResIcon r={2} />
              <ResIcon r={3} />
              <ResIcon r={4} />
            </span>
            <span class="muted" style={{ flex: '1', 'text-align': 'right' }}>
              骑士(3+争最大骑士+2分) / 修路×2 / 丰收×2 / 垄断 / 暗分+1
            </span>
          </div>
        </div>
      </div>

      <div
        class="rules-box small"
        data-testid="citadels-cost-card"
        style={{
          gap: '5px',
          border: isCitadelsTab() ? '1px solid rgba(240, 180, 41, 0.55)' : '1px solid rgba(255,255,255,0.08)',
          background: isCitadelsTab() ? 'rgba(240, 180, 41, 0.11)' : 'rgba(255,255,255,0.03)',
        }}
      >
        <div class="row" style={{ 'justify-content': 'space-between' }}>
          <b>城邦铁卫与三系科技（城邦与铁卫专属）</b>
          <Show when={isCitadelsTab()}>
            <span class="badge">高亮生效</span>
          </Show>
        </div>
        <div class="row" style={{ gap: '10px 14px' }}>
          <span style={{ display: 'inline-flex', 'align-items': 'center', gap: '4px' }}>
            招募/晋升铁卫：
            <ResIcon r={2} />
            <ResIcon r={4} />
          </span>
          <span style={{ display: 'inline-flex', 'align-items': 'center', gap: '4px' }}>
            激活铁卫：
            <ResIcon r={3} />
          </span>
        </div>
        <div class="muted" style={{ 'line-height': '1.5' }}>
          城市三系升级（升至 Lv.N 消耗 N 张商品：商贸 <CommIcon c={1} /> / 军政 <CommIcon c={2} /> / 学术{' '}
          <CommIcon c={0} />
          ）— Lv3 解锁商馆 2:1 / 帅印 3阶铁卫 / 水渠轮空拿牌；Lv4/Lv5 争 +2分大都会且免疫黑帆降级。
        </div>
      </div>

      <footer>
        <button type="button" class="primary" onClick={props.onClose}>
          知道了
        </button>
      </footer>
    </Modal>
  );
}

export const RulesDrawer = RulesModal;
