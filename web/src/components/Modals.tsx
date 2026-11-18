import { createMemo, createSignal, For, Show } from 'solid-js';
import type { Game } from '../game/controller';
import type { ReplayRecord, ResourceSet, TradeResponse } from '../generated/proto';
import { RESOURCES, RESOURCE_NAME, setTotal } from '../game/text';
import { Modal, ResIcon, SeatDot, SetView, Stepper } from './bits';
import { downloadReplay } from './Hud';

const empty = (): ResourceSet => [0, 0, 0, 0, 0];
const withCount = (s: ResourceSet, i: number, v: number): ResourceSet => s.map((x, j) => (j === i ? v : x)) as ResourceSet;

export function DiscardModal(props: { game: Game }) {
  const g = props.game;
  const need = () => (g.me === null ? 0 : g.view().pendingDiscard[g.me]);
  const hand = () => g.view().me?.hand ?? empty();
  const [pick, setPick] = createSignal<ResourceSet>(empty());
  const total = () => setTotal(pick());
  return (
    <Modal title={`弃掉 ${need()} 张牌`}>
      <p class="muted">掷出了 7，你的手牌超过 7 张，需要弃掉一半（向下取整）。</p>
      <div class="pickers">
        <For each={RESOURCES}>
          {(_, i) => (
            <Stepper
              label={<ResIcon r={i()} size="md" />}
              value={pick()[i()]}
              max={Math.min(hand()[i()], pick()[i()] + need() - total())}
              onChange={(v) => setPick(withCount(pick(), i(), v))}
            />
          )}
        </For>
      </div>
      <footer>
        <span class="muted">
          已选 {total()} / {need()}
        </span>
        <button class="primary" disabled={total() !== need()} onClick={() => g.actSet('Discard', pick()) && setPick(empty())}>
          确认弃牌
        </button>
      </footer>
    </Modal>
  );
}

export function VictimModal(props: { game: Game }) {
  const g = props.game;
  return (
    <Show when={g.victimChoice()}>
      {(c) => (
        <Modal title="抢谁的牌？" onClose={g.clearVictimChoice}>
          <div class="choices">
            <For each={c().victims}>
              {(q) => (
                <button
                  class="choice"
                  onClick={() => {
                    g.act((a) => a.type === 'MoveRobber' && a.value.tile === c().tile && a.value.victim === q);
                    g.clearVictimChoice();
                  }}
                >
                  <SeatDot seat={q} /> {g.name(q)}
                  <span class="muted">{g.view().players[q].resourceCount} 张牌</span>
                </button>
              )}
            </For>
          </div>
        </Modal>
      )}
    </Show>
  );
}

export function YearOfPlentyModal(props: { game: Game; onClose: () => void }) {
  const g = props.game;
  const [pick, setPick] = createSignal<ResourceSet>(empty());
  const want = () => Math.min(2, setTotal(g.view().bank));
  return (
    <Modal title="丰收：从银行拿 2 张资源" onClose={props.onClose}>
      <div class="pickers">
        <For each={RESOURCES}>
          {(_, i) => (
            <Stepper
              label={<ResIcon r={i()} size="md" />}
              value={pick()[i()]}
              max={Math.min(g.view().bank[i()], pick()[i()] + want() - setTotal(pick()))}
              onChange={(v) => setPick(withCount(pick(), i(), v))}
            />
          )}
        </For>
      </div>
      <footer>
        <button
          class="primary"
          disabled={setTotal(pick()) !== want()}
          onClick={() => g.actSet('PlayYearOfPlenty', pick()) && props.onClose()}
        >
          拿取
        </button>
      </footer>
    </Modal>
  );
}

export function MonopolyModal(props: { game: Game; onClose: () => void }) {
  const g = props.game;
  return (
    <Modal title="垄断：选择一种资源" onClose={props.onClose}>
      <p class="muted">所有对手都要交出手中全部这种资源。</p>
      <div class="choices row">
        <For each={RESOURCES}>
          {(r, i) => (
            <button class="choice" onClick={() => g.act((a) => a.type === 'PlayMonopoly' && a.value === r) && props.onClose()}>
              <ResIcon r={i()} size="md" /> {RESOURCE_NAME[r]}
            </button>
          )}
        </For>
      </div>
    </Modal>
  );
}

const RESPONSE_TEXT: Record<TradeResponse, string> = { Pending: '考虑中…', Accepted: '接受', Rejected: '拒绝' };

export function TradeModal(props: { game: Game; onClose: () => void }) {
  const g = props.game;
  const [tab, setTab] = createSignal<'bank' | 'players'>(g.view().trade ? 'players' : 'bank');
  const [give, setGive] = createSignal<ResourceSet>(empty());
  const [get, setGet] = createSignal<ResourceSet>(empty());
  const [bankGive, setBankGive] = createSignal<number | null>(null);
  const me = () => g.view().me!;
  const offer = () => g.view().trade;

  const maritime = createMemo(() =>
    new Set(g.legal().actions.flatMap((a) => (a.type === 'MaritimeTrade' ? [`${a.value.give}>${a.value.get}`] : []))),
  );

  return (
    <Modal title="交易" onClose={props.onClose} wide>
      <div class="tabs">
        <button classList={{ on: tab() === 'bank' }} onClick={() => setTab('bank')}>
          与银行
        </button>
        <button classList={{ on: tab() === 'players' }} onClick={() => setTab('players')}>
          与玩家
        </button>
      </div>

      <Show when={tab() === 'bank'}>
        <p class="muted">选择要给出的资源（按你拥有的港口自动使用最优比例），再选择想要的资源。</p>
        <div class="bank-grid">
          <For each={RESOURCES}>
            {(r, i) => {
              const ratio = () => me().tradeRatios[i()];
              const can = () => me().hand[i()] >= ratio();
              return (
                <button class="choice" classList={{ on: bankGive() === i() }} disabled={!can()} onClick={() => setBankGive(i())}>
                  <ResIcon r={i()} size="md" />
                  <span>
                    {ratio()} {RESOURCE_NAME[r]} → 1
                  </span>
                  <span class="muted">有 {me().hand[i()]}</span>
                </button>
              );
            }}
          </For>
        </div>
        <Show when={bankGive() !== null}>
          <p>换成：</p>
          <div class="choices row">
            <For each={RESOURCES}>
              {(r, i) => (
                <button
                  class="choice"
                  disabled={!maritime().has(`${RESOURCES[bankGive()!]}>${r}`)}
                  onClick={() => {
                    g.act((a) => a.type === 'MaritimeTrade' && a.value.give === RESOURCES[bankGive()!] && a.value.get === r);
                    if (me().hand[bankGive()!] < me().tradeRatios[bankGive()!]) setBankGive(null);
                  }}
                >
                  <ResIcon r={i()} size="md" /> {RESOURCE_NAME[r]}
                </button>
              )}
            </For>
          </div>
        </Show>
      </Show>

      <Show when={tab() === 'players'}>
        <Show
          when={offer()}
          fallback={
            <>
              <div class="trade-cols">
                <div>
                  <h4>你给出</h4>
                  <For each={RESOURCES}>
                    {(_, i) => (
                      <Stepper
                        label={<ResIcon r={i()} size="md" />}
                        value={give()[i()]}
                        max={me().hand[i()]}
                        onChange={(v) => {
                          setGive(withCount(give(), i(), v));
                          if (v > 0) setGet(withCount(get(), i(), 0));
                        }}
                      />
                    )}
                  </For>
                </div>
                <div>
                  <h4>你想要</h4>
                  <For each={RESOURCES}>
                    {(_, i) => (
                      <Stepper
                        label={<ResIcon r={i()} size="md" />}
                        value={get()[i()]}
                        max={9}
                        onChange={(v) => {
                          setGet(withCount(get(), i(), v));
                          if (v > 0) setGive(withCount(give(), i(), 0));
                        }}
                      />
                    )}
                  </For>
                </div>
              </div>
              <footer>
                <span class="muted">双方都至少要给出一张牌</span>
                <button
                  class="primary"
                  disabled={setTotal(give()) === 0 || setTotal(get()) === 0}
                  onClick={() => g.proposeTrade(give(), get())}
                >
                  发起报价
                </button>
              </footer>
            </>
          }
        >
          {(o) => (
            <>
              <p class="offer">
                你给出 <SetView set={o().give} />，换取 <SetView set={o().get} />
              </p>
              <div class="choices">
                <For each={g.view().players.filter((p) => p.seat !== g.me)}>
                  {(p) => {
                    const resp = () => o().responses[p.seat];
                    return (
                      <div class="response">
                        <SeatDot seat={p.seat} /> {g.name(p.seat)}
                        <span class={`resp resp-${resp().toLowerCase()}`}>{RESPONSE_TEXT[resp()]}</span>
                        <Show when={g.legal().actions.some((a) => a.type === 'ConfirmTrade' && a.value.partner === p.seat)}>
                          <button class="primary" onClick={() => g.act((a) => a.type === 'ConfirmTrade' && a.value.partner === p.seat)}>
                            成交
                          </button>
                        </Show>
                      </div>
                    );
                  }}
                </For>
              </div>
              <footer>
                <button onClick={() => g.act((a) => a.type === 'CancelTrade')}>撤回报价</button>
              </footer>
            </>
          )}
        </Show>
      </Show>
    </Modal>
  );
}

export function GameOverModal(props: {
  game: Game;
  onAgain?: () => void;
  onLobby: () => void;
  onWatchReplay?: (record: ReplayRecord, link: string | null) => void;
}) {
  const g = props.game;
  const ranking = () =>
    g
      .view()
      .players.map((p) => ({ seat: p.seat, points: g.view().finalPoints?.[p.seat] ?? p.publicVictoryPoints }))
      .sort((a, b) => b.points - a.points);
  return (
    <Modal title={g.view().winner === g.me ? '你赢了！' : `${g.name(g.view().winner!)}获胜`}>
      <ol class="ranking">
        <For each={ranking()}>
          {(r) => (
            <li classList={{ winner: r.seat === g.view().winner }}>
              <SeatDot seat={r.seat} /> {g.name(r.seat)}
              <b>{r.points} 分</b>
            </li>
          )}
        </For>
      </ol>
      <p class="muted">共 {g.view().turn} 个回合</p>
      <Show when={g.exportReplay}>
        {(load) => {
          const [busy, setBusy] = createSignal(false);
          const withRecord = async (use: (r: ReplayRecord) => void) => {
            if (busy()) return;
            setBusy(true);
            try {
              const record = await load()();
              if (record) use(record);
              else g.showToast('回放还没准备好，请稍后再试');
            } finally {
              setBusy(false);
            }
          };
          return (
            <div class="row replay-actions">
              <Show when={props.onWatchReplay}>
                {(watch) => (
                  <button disabled={busy()} onClick={() => void withRecord((r) => watch()(r, g.replayLink?.() ?? null))}>
                    观看回放
                  </button>
                )}
              </Show>
              <button disabled={busy()} onClick={() => void withRecord(downloadReplay)}>
                下载回放
              </button>
            </div>
          );
        }}
      </Show>
      <footer>
        <button onClick={props.onLobby}>返回大厅</button>
        <Show when={props.onAgain}>
          {(again) => (
            <button class="primary" onClick={() => again()()}>
              再来一局
            </button>
          )}
        </Show>
      </footer>
    </Modal>
  );
}

/** An offer from another player, shown to everyone else while it is open. */
export function IncomingTrade(props: { game: Game }) {
  const g = props.game;
  const offer = () => {
    const v = g.view();
    return g.me !== null && v.trade && v.current !== g.me ? v.trade : null;
  };
  const canAccept = () => g.legal().actions.some((a) => a.type === 'RespondTrade' && a.value.accept);
  return (
    <Show when={offer()}>
      {(o) => {
        const response = () => o().responses[g.me!];
        const from = () => g.view().current;
        return (
          <div class="panel incoming" role="dialog" aria-label="交易报价">
            <p>
              <SeatDot seat={from()} /> <b>{g.name(from())}</b> 想和你交易
            </p>
            <p class="offer">
              你得到 <SetView set={o().give} />，你给出 <SetView set={o().get} />
            </p>
            <Show
              when={response() === 'Pending'}
              fallback={
                <div class="row">
                  <span class="muted">{response() === 'Accepted' ? '你已接受，等待对方确认' : '你已拒绝'}</span>
                  <Show when={response() === 'Accepted'}>
                    <button onClick={() => g.act((a) => a.type === 'RespondTrade' && !a.value.accept)}>撤销</button>
                  </Show>
                </div>
              }
            >
              <div class="row">
                <button
                  class="primary"
                  disabled={!canAccept()}
                  title={canAccept() ? '' : '你没有足够的牌'}
                  onClick={() => g.act((a) => a.type === 'RespondTrade' && a.value.accept)}
                >
                  接受
                </button>
                <button onClick={() => g.act((a) => a.type === 'RespondTrade' && !a.value.accept)}>拒绝</button>
              </div>
            </Show>
          </div>
        );
      }}
    </Show>
  );
}
