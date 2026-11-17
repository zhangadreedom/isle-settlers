//! Rule validation (`check`) and state transitions (`execute`).

use crate::action::{Action, Event, EventSink, RuleError};
use crate::resources::{Resource, ResourceSet, cost};
use crate::state::*;
use crate::topology::*;

use RuleError as E;

impl GameState {
    /// Validates and applies `action` for seat `player`.
    #[inline]
    pub fn apply(&mut self, player: u8, action: Action) -> Result<(), RuleError> {
        self.apply_with(player, action, &mut ())
    }

    /// Like [`apply`](Self::apply), reporting what happened to `sink`.
    /// On error the state is unchanged.
    pub fn apply_with<S: EventSink + ?Sized>(
        &mut self,
        player: u8,
        action: Action,
        sink: &mut S,
    ) -> Result<(), RuleError> {
        self.check(player, action)?;
        self.execute(player, action, sink);
        Ok(())
    }

    /// `Ok` if `player` may perform `action` now.
    pub fn check(&self, player: u8, action: Action) -> Result<(), RuleError> {
        if self.phase == Phase::GameOver {
            return Err(E::GameOver);
        }
        if player as usize >= self.num_players() {
            return Err(E::InvalidPlayer);
        }
        let ps = &self.players[player as usize];

        // Actions available to players other than the current one.
        match action {
            Action::Discard(set) => {
                if self.phase != Phase::Discard {
                    return Err(E::WrongPhase);
                }
                let owed = self.pending_discard[player as usize];
                if owed == 0 {
                    return Err(E::NothingToDiscard);
                }
                if set.total() != owed as u32 {
                    return Err(E::WrongDiscardAmount);
                }
                return if ps.hand.contains(&set) {
                    Ok(())
                } else {
                    Err(E::InsufficientResources)
                };
            }
            Action::RespondTrade { accept } => {
                if self.phase != Phase::Main {
                    return Err(E::WrongPhase);
                }
                let offer = self.trade.ok_or(E::NoOpenTrade)?;
                if player == self.current {
                    return Err(E::InvalidTrade);
                }
                if accept && !ps.hand.contains(&offer.get) {
                    return Err(E::InsufficientResources);
                }
                return Ok(());
            }
            _ => {}
        }

        if player != self.current {
            return Err(E::NotYourTurn);
        }

        match action {
            Action::RollDice => self.require_phase(Phase::PreRoll),

            Action::BuildRoad(e) => {
                if e as usize >= NUM_EDGES {
                    return Err(E::InvalidEdge);
                }
                let bit = 1u128 << e;
                if self.occupied_edges() & bit != 0 {
                    return Err(E::Occupied);
                }
                match self.phase {
                    Phase::SetupRoad { vertex } => {
                        if VERTEX_EDGE_MASK[vertex as usize] & bit == 0 {
                            return Err(E::NotConnected);
                        }
                        Ok(())
                    }
                    Phase::Main | Phase::RoadBuilding { .. } => {
                        if ps.roads_left == 0 {
                            return Err(E::NoPiecesLeft);
                        }
                        if self.phase == Phase::Main && !ps.hand.contains(&cost::ROAD) {
                            return Err(E::InsufficientResources);
                        }
                        if self.road_spots(player) & bit == 0 {
                            return Err(E::NotConnected);
                        }
                        Ok(())
                    }
                    _ => Err(E::WrongPhase),
                }
            }

            Action::BuildSettlement(v) => {
                let setup = match self.phase {
                    Phase::SetupSettlement => true,
                    Phase::Main => false,
                    _ => return Err(E::WrongPhase),
                };
                if v as usize >= NUM_VERTICES {
                    return Err(E::InvalidVertex);
                }
                let bit = 1u64 << v;
                let occ = self.occupied_vertices();
                if occ & bit != 0 {
                    return Err(E::Occupied);
                }
                if VERTEX_ADJ_MASK[v as usize] & occ != 0 {
                    return Err(E::TooCloseToBuilding);
                }
                if ps.settlements_left == 0 {
                    return Err(E::NoPiecesLeft);
                }
                if !setup {
                    if !ps.hand.contains(&cost::SETTLEMENT) {
                        return Err(E::InsufficientResources);
                    }
                    if VERTEX_EDGE_MASK[v as usize] & self.roads[player as usize] == 0 {
                        return Err(E::NotConnected);
                    }
                }
                Ok(())
            }

            Action::BuildCity(v) => {
                self.require_phase(Phase::Main)?;
                if v as usize >= NUM_VERTICES {
                    return Err(E::InvalidVertex);
                }
                if self.settlements[player as usize] & (1u64 << v) == 0 {
                    return Err(E::NotYourSettlement);
                }
                if ps.cities_left == 0 {
                    return Err(E::NoPiecesLeft);
                }
                if !ps.hand.contains(&cost::CITY) {
                    return Err(E::InsufficientResources);
                }
                Ok(())
            }

            Action::BuyDevCard => {
                self.require_phase(Phase::Main)?;
                if self.dev_deck_len == 0 {
                    return Err(E::DeckEmpty);
                }
                if !ps.hand.contains(&cost::DEV_CARD) {
                    return Err(E::InsufficientResources);
                }
                Ok(())
            }

            Action::PlayKnight => self.check_dev_card(player, DevCard::Knight),
            Action::PlayRoadBuilding => {
                self.check_dev_card(player, DevCard::RoadBuilding)?;
                if ps.roads_left == 0 || self.road_spots(player) == 0 {
                    return Err(E::NoRoadSpot);
                }
                Ok(())
            }
            Action::PlayYearOfPlenty(set) => {
                self.check_dev_card(player, DevCard::YearOfPlenty)?;
                let want = self.bank.total().min(2);
                if want == 0 || set.total() != want {
                    return Err(E::InvalidTrade);
                }
                if !self.bank.contains(&set) {
                    return Err(E::BankShort);
                }
                Ok(())
            }
            Action::PlayMonopoly(_) => self.check_dev_card(player, DevCard::Monopoly),

            Action::MoveRobber { tile, victim } => {
                self.require_phase(Phase::MoveRobber)?;
                if tile as usize >= NUM_TILES {
                    return Err(E::InvalidTile);
                }
                if tile == self.robber {
                    return Err(E::RobberMustMove);
                }
                let victims = self.robber_victims(tile, player);
                match victim {
                    None if victims == 0 => Ok(()),
                    Some(q) if (q as usize) < self.num_players() && victims & (1 << q) != 0 => {
                        Ok(())
                    }
                    _ => Err(E::InvalidVictim),
                }
            }

            Action::MaritimeTrade { give, get } => {
                self.require_phase(Phase::Main)?;
                if give == get {
                    return Err(E::InvalidTrade);
                }
                if ps.hand[give] < self.trade_ratio(player, give) {
                    return Err(E::InsufficientResources);
                }
                if self.bank[get] == 0 {
                    return Err(E::BankShort);
                }
                Ok(())
            }

            Action::ProposeTrade { give, get } => {
                self.require_phase(Phase::Main)?;
                if give.is_empty() || get.is_empty() || give.overlaps(&get) {
                    return Err(E::InvalidTrade);
                }
                if !ps.hand.contains(&give) {
                    return Err(E::InsufficientResources);
                }
                Ok(())
            }

            Action::ConfirmTrade { partner } => {
                self.require_phase(Phase::Main)?;
                let offer = self.trade.ok_or(E::NoOpenTrade)?;
                if partner as usize >= self.num_players() || partner == player {
                    return Err(E::InvalidPlayer);
                }
                if offer.responses[partner as usize] != TradeResponse::Accepted {
                    return Err(E::PartnerHasNotAccepted);
                }
                if !ps.hand.contains(&offer.give)
                    || !self.players[partner as usize].hand.contains(&offer.get)
                {
                    return Err(E::InsufficientResources);
                }
                Ok(())
            }

            Action::CancelTrade => {
                self.require_phase(Phase::Main)?;
                self.trade.map(|_| ()).ok_or(E::NoOpenTrade)
            }

            Action::EndTurn => self.require_phase(Phase::Main),

            Action::Discard(_) | Action::RespondTrade { .. } => unreachable!("handled above"),
        }
    }

    #[inline]
    fn require_phase(&self, phase: Phase) -> Result<(), RuleError> {
        if self.phase == phase {
            Ok(())
        } else {
            Err(E::WrongPhase)
        }
    }

    fn check_dev_card(&self, player: u8, card: DevCard) -> Result<(), RuleError> {
        if !matches!(self.phase, Phase::PreRoll | Phase::Main) {
            return Err(E::WrongPhase);
        }
        if self.dev_card_played {
            return Err(E::DevCardAlreadyPlayed);
        }
        if self.players[player as usize].dev_cards[card.index()] == 0 {
            return Err(E::NoSuchCard);
        }
        Ok(())
    }

    /// Applies a validated action.
    fn execute<S: EventSink + ?Sized>(&mut self, player: u8, action: Action, sink: &mut S) {
        let p = player as usize;

        // Any non-trade action by the current player withdraws an open offer.
        if self.trade.is_some()
            && player == self.current
            && !matches!(
                action,
                Action::ProposeTrade { .. } | Action::ConfirmTrade { .. } | Action::CancelTrade
            )
        {
            self.trade = None;
            sink.emit(Event::TradeCancelled { player });
        }

        match action {
            Action::RollDice => {
                let dice = (self.rng.die(), self.rng.die());
                self.dice = Some(dice);
                sink.emit(Event::DiceRolled { player, dice });
                let sum = dice.0 + dice.1;
                if sum == 7 {
                    let mut any = false;
                    for q in 0..self.num_players() {
                        let n = self.players[q].hand.total();
                        if n > DISCARD_LIMIT {
                            self.pending_discard[q] = (n / 2) as u8;
                            any = true;
                        }
                    }
                    self.phase = if any {
                        Phase::Discard
                    } else {
                        Phase::MoveRobber
                    };
                } else {
                    self.produce(sum, sink);
                    self.phase = Phase::Main;
                }
            }

            Action::BuildRoad(e) => {
                let free = !matches!(self.phase, Phase::Main);
                if !free {
                    self.pay(p, cost::ROAD);
                }
                self.roads[p] |= 1u128 << e;
                self.players[p].roads_left -= 1;
                sink.emit(Event::RoadBuilt { player, edge: e });
                self.update_longest_road_after_build(player, sink);
                match self.phase {
                    Phase::SetupRoad { .. } => self.advance_setup(),
                    Phase::RoadBuilding { remaining } => {
                        let remaining = remaining - 1;
                        self.phase = if remaining == 0
                            || self.players[p].roads_left == 0
                            || self.road_spots(player) == 0
                        {
                            self.after_interrupt()
                        } else {
                            Phase::RoadBuilding { remaining }
                        };
                    }
                    _ => {}
                }
            }

            Action::BuildSettlement(v) => {
                let setup = self.phase == Phase::SetupSettlement;
                if !setup {
                    self.pay(p, cost::SETTLEMENT);
                }
                self.settlements[p] |= 1u64 << v;
                self.players[p].settlements_left -= 1;
                self.players[p].harbors |= self.board.vertex_harbor[v as usize];
                sink.emit(Event::SettlementBuilt { player, vertex: v });
                if setup {
                    if self.is_second_setup_round() {
                        let mut got = ResourceSet::EMPTY;
                        for &t in &VERTEX_TILES[v as usize] {
                            if t == NONE {
                                break;
                            }
                            if let Some(r) = self.board.terrain[t as usize].resource() {
                                got[r] += 1;
                            }
                        }
                        self.bank -= got;
                        self.players[p].hand += got;
                        sink.emit(Event::StartingResources {
                            player,
                            resources: got,
                        });
                    }
                    self.phase = Phase::SetupRoad { vertex: v };
                } else {
                    self.update_longest_road_after_break(v, player, sink);
                }
            }

            Action::BuildCity(v) => {
                self.pay(p, cost::CITY);
                let bit = 1u64 << v;
                self.settlements[p] &= !bit;
                self.cities[p] |= bit;
                self.players[p].settlements_left += 1;
                self.players[p].cities_left -= 1;
                sink.emit(Event::CityBuilt { player, vertex: v });
            }

            Action::BuyDevCard => {
                self.pay(p, cost::DEV_CARD);
                self.dev_deck_len -= 1;
                let card = self.dev_deck[self.dev_deck_len as usize];
                if card == DevCard::VictoryPoint {
                    self.players[p].dev_cards[card.index()] += 1;
                } else {
                    self.players[p].new_dev_cards[card.index()] += 1;
                }
                sink.emit(Event::DevCardBought {
                    player,
                    card: Some(card),
                });
            }

            Action::PlayKnight => {
                self.consume_dev_card(player, DevCard::Knight, sink);
                self.update_largest_army(player, sink);
                self.phase = Phase::MoveRobber;
            }

            Action::PlayRoadBuilding => {
                self.consume_dev_card(player, DevCard::RoadBuilding, sink);
                self.phase = Phase::RoadBuilding {
                    remaining: self.players[p].roads_left.min(2),
                };
            }

            Action::PlayYearOfPlenty(set) => {
                self.consume_dev_card(player, DevCard::YearOfPlenty, sink);
                self.bank -= set;
                self.players[p].hand += set;
                sink.emit(Event::YearOfPlentyTaken {
                    player,
                    resources: set,
                });
            }

            Action::PlayMonopoly(r) => {
                self.consume_dev_card(player, DevCard::Monopoly, sink);
                let mut taken = [0u8; MAX_PLAYERS];
                for q in 0..self.num_players() {
                    if q != p {
                        taken[q] = std::mem::take(&mut self.players[q].hand[r]);
                        self.players[p].hand[r] += taken[q];
                    }
                }
                sink.emit(Event::MonopolyTaken {
                    player,
                    resource: r,
                    taken,
                });
            }

            Action::MoveRobber { tile, victim } => {
                self.robber = tile;
                sink.emit(Event::RobberMoved { player, tile });
                if let Some(q) = victim {
                    let hand = self.players[q as usize].hand;
                    let idx = self.rng.below(hand.total());
                    let r = hand.nth_card(idx).expect("victim has cards");
                    self.players[q as usize].hand[r] -= 1;
                    self.players[p].hand[r] += 1;
                    sink.emit(Event::Stolen {
                        thief: player,
                        victim: q,
                        resource: Some(r),
                    });
                }
                self.phase = self.after_interrupt();
            }

            Action::Discard(set) => {
                self.players[p].hand -= set;
                self.bank += set;
                self.pending_discard[p] = 0;
                sink.emit(Event::Discarded {
                    player,
                    resources: set,
                });
                if self.pending_discard.iter().all(|&d| d == 0) {
                    self.phase = Phase::MoveRobber;
                }
            }

            Action::MaritimeTrade { give, get } => {
                let ratio = self.trade_ratio(player, give);
                let gave = ResourceSet::single(give, ratio);
                self.players[p].hand -= gave;
                self.bank += gave;
                self.bank[get] -= 1;
                self.players[p].hand[get] += 1;
                sink.emit(Event::MaritimeTraded {
                    player,
                    give: gave,
                    get,
                });
            }

            Action::ProposeTrade { give, get } => {
                let mut responses = [TradeResponse::Pending; MAX_PLAYERS];
                responses[p] = TradeResponse::Rejected;
                for q in self.num_players()..MAX_PLAYERS {
                    responses[q] = TradeResponse::Rejected;
                }
                self.trade = Some(TradeOffer {
                    give,
                    get,
                    responses,
                });
                sink.emit(Event::TradeProposed { player, give, get });
            }

            Action::RespondTrade { accept } => {
                if let Some(offer) = self.trade.as_mut() {
                    offer.responses[p] = if accept {
                        TradeResponse::Accepted
                    } else {
                        TradeResponse::Rejected
                    };
                }
                sink.emit(Event::TradeResponded { player, accept });
            }

            Action::ConfirmTrade { partner } => {
                let offer = self.trade.take().expect("checked");
                let q = partner as usize;
                self.players[p].hand -= offer.give;
                self.players[p].hand += offer.get;
                self.players[q].hand -= offer.get;
                self.players[q].hand += offer.give;
                sink.emit(Event::TradeExecuted {
                    player,
                    partner,
                    give: offer.give,
                    get: offer.get,
                });
            }

            Action::CancelTrade => {
                self.trade = None;
                sink.emit(Event::TradeCancelled { player });
            }

            Action::EndTurn => {
                let ps = &mut self.players[p];
                for c in 0..5 {
                    ps.dev_cards[c] += std::mem::take(&mut ps.new_dev_cards[c]);
                }
                self.dev_card_played = false;
                self.dice = None;
                self.turn += 1;
                self.current = ((p + 1) % self.num_players()) as u8;
                self.phase = Phase::PreRoll;
                sink.emit(Event::TurnEnded { player });
            }
        }

        // A player can only win on their own turn; this also covers points
        // gained during an opponent's turn, which count once theirs begins.
        if self.phase != Phase::GameOver && !self.is_setup() {
            let vp = self.victory_points(self.current);
            if vp >= self.config.victory_points {
                self.winner = Some(self.current);
                self.phase = Phase::GameOver;
                self.trade = None;
                sink.emit(Event::GameWon {
                    player: self.current,
                    points: vp,
                });
            }
        }
    }

    #[inline]
    pub fn is_setup(&self) -> bool {
        matches!(self.phase, Phase::SetupSettlement | Phase::SetupRoad { .. })
    }

    #[inline]
    fn pay(&mut self, p: usize, price: ResourceSet) {
        self.players[p].hand -= price;
        self.bank += price;
    }

    /// Phase to return to after the robber or Road Building interrupts a turn.
    #[inline]
    fn after_interrupt(&self) -> Phase {
        if self.dice.is_some() {
            Phase::Main
        } else {
            Phase::PreRoll
        }
    }

    fn advance_setup(&mut self) {
        self.setup_step += 1;
        if self.setup_step as usize == 2 * self.num_players() {
            self.current = 0;
            self.phase = Phase::PreRoll;
        } else {
            self.current = self.setup_player(self.setup_step);
            self.phase = Phase::SetupSettlement;
        }
    }

    fn consume_dev_card<S: EventSink + ?Sized>(&mut self, player: u8, card: DevCard, sink: &mut S) {
        let ps = &mut self.players[player as usize];
        ps.dev_cards[card.index()] -= 1;
        ps.played_dev_cards[card.index()] += 1;
        self.dev_card_played = true;
        sink.emit(Event::DevCardPlayed { player, card });
    }

    /// Distributes resources for a dice roll. If the bank cannot pay every
    /// claim on a resource, nobody receives it, unless only one player is
    /// owed that resource, who then receives what is left.
    fn produce<S: EventSink + ?Sized>(&mut self, roll: u8, sink: &mut S) {
        let n = self.num_players();
        let mut owed = [ResourceSet::EMPTY; MAX_PLAYERS];
        for &t in &self.board.tiles_by_number[roll as usize] {
            if t == NONE || t == self.robber {
                continue;
            }
            let Some(r) = self.board.terrain[t as usize].resource() else {
                continue;
            };
            let tv = TILE_VERTEX_MASK[t as usize];
            for q in 0..n {
                owed[q][r] += (self.settlements[q] & tv).count_ones() as u8
                    + 2 * (self.cities[q] & tv).count_ones() as u8;
            }
        }
        for r in Resource::ALL {
            let total: u32 = (0..n).map(|q| owed[q][r] as u32).sum();
            if total == 0 || total <= self.bank[r] as u32 {
                continue;
            }
            let claimants = (0..n).filter(|&q| owed[q][r] > 0).count();
            for q in 0..n {
                owed[q][r] = if claimants == 1 {
                    owed[q][r].min(self.bank[r])
                } else {
                    0
                };
            }
        }
        for q in 0..n {
            if !owed[q].is_empty() {
                self.bank -= owed[q];
                self.players[q].hand += owed[q];
                sink.emit(Event::ResourcesProduced {
                    player: q as u8,
                    resources: owed[q],
                });
            }
        }
    }

    fn update_largest_army<S: EventSink + ?Sized>(&mut self, player: u8, sink: &mut S) {
        let k = self.players[player as usize].knights_played();
        if k < LARGEST_ARMY_MIN {
            return;
        }
        let takes = match self.largest_army_owner {
            None => true,
            Some(h) => h != player && k > self.players[h as usize].knights_played(),
        };
        if takes {
            self.largest_army_owner = Some(player);
            sink.emit(Event::LargestArmyChanged {
                owner: Some(player),
            });
        }
    }

    fn update_longest_road_after_build<S: EventSink + ?Sized>(&mut self, player: u8, sink: &mut S) {
        let len = self.compute_longest_road(player);
        self.players[player as usize].longest_road = len;
        let owner = match self.longest_road_owner {
            Some(h) if h == player => Some(h),
            Some(h) => {
                if len > self.players[h as usize].longest_road {
                    Some(player)
                } else {
                    Some(h)
                }
            }
            None => {
                let sole_longest = (0..self.num_players())
                    .all(|q| q == player as usize || self.players[q].longest_road < len);
                (len >= LONGEST_ROAD_MIN && sole_longest).then_some(player)
            }
        };
        if owner != self.longest_road_owner {
            self.longest_road_owner = owner;
            sink.emit(Event::LongestRoadChanged { owner });
        }
    }

    /// A settlement at `v` may split opponents' roads. The holder keeps the
    /// card while still (co-)longest; otherwise it goes to the single longest
    /// road of 5+, or is set aside on a tie.
    fn update_longest_road_after_break<S: EventSink + ?Sized>(
        &mut self,
        v: u8,
        builder: u8,
        sink: &mut S,
    ) {
        let edges_at_v = VERTEX_EDGE_MASK[v as usize];
        let mut changed = false;
        for q in 0..self.num_players() as u8 {
            if q != builder && (self.roads[q as usize] & edges_at_v).count_ones() >= 2 {
                let len = self.compute_longest_road(q);
                if len != self.players[q as usize].longest_road {
                    self.players[q as usize].longest_road = len;
                    changed = true;
                }
            }
        }
        if !changed {
            return;
        }
        let max = (0..self.num_players())
            .map(|q| self.players[q].longest_road)
            .max()
            .unwrap_or(0);
        if let Some(h) = self.longest_road_owner {
            let len = self.players[h as usize].longest_road;
            if len >= LONGEST_ROAD_MIN && len == max {
                return;
            }
        }
        let mut leaders =
            (0..self.num_players() as u8).filter(|&q| self.players[q as usize].longest_road == max);
        let owner = match (leaders.next(), leaders.next()) {
            (Some(q), None) if max >= LONGEST_ROAD_MIN => Some(q),
            _ => None,
        };
        if owner != self.longest_road_owner {
            self.longest_road_owner = owner;
            sink.emit(Event::LongestRoadChanged { owner });
        }
    }
}
