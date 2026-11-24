//! Rule validation (`check`) and state transitions (`execute`).

use crate::action::{Action, Event, EventSink, RuleError};
use crate::resources::{Resource, ResourceSet, cost};
use crate::state::*;
use crate::topology::*;

use RuleError as E;

impl GameState {
    /// Validates and applies `action` for seat `player`. Not inlined, so
    /// the event-less instantiation exists once rather than once per crate.
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
            Action::ChooseGold(set) => {
                if !matches!(self.phase, Phase::Gold { .. }) {
                    return Err(E::WrongPhase);
                }
                let owed = self.pending_gold[player as usize];
                if owed == 0 {
                    return Err(E::NoGoldOwed);
                }
                if set.total() != (owed as u32).min(self.bank.total()) {
                    return Err(E::WrongGoldAmount);
                }
                return if self.bank.contains(&set) {
                    Ok(())
                } else {
                    Err(E::BankShort)
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

        if player != self.active_player() {
            return Err(E::NotYourTurn);
        }
        let topo = self.topo();
        let special = matches!(self.phase, Phase::SpecialBuild { .. });

        match action {
            Action::RollDice => self.require_phase(Phase::PreRoll),

            Action::BuildRoad(e) => {
                if e as usize >= topo.num_edges {
                    return Err(E::InvalidEdge);
                }
                let bit = 1u128 << e;
                if self.occupied_edges() & bit != 0 {
                    return Err(E::Occupied);
                }
                if self.board.land_edges & bit == 0 {
                    return Err(E::WrongGround);
                }
                match self.phase {
                    Phase::SetupRoad { vertex } => {
                        if topo.vertex_edge_mask[vertex as usize] & bit == 0 {
                            return Err(E::NotConnected);
                        }
                        Ok(())
                    }
                    Phase::Main | Phase::RoadBuilding { .. } | Phase::SpecialBuild { .. } => {
                        if ps.roads_left == 0 {
                            return Err(E::NoPiecesLeft);
                        }
                        let paid = !matches!(self.phase, Phase::RoadBuilding { .. });
                        if paid && !ps.hand.contains(&cost::ROAD) {
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
                    Phase::Main | Phase::SpecialBuild { .. } => false,
                    _ => return Err(E::WrongPhase),
                };
                if v as usize >= topo.num_vertices {
                    return Err(E::InvalidVertex);
                }
                let bit = 1u128 << v;
                let occ = self.occupied_vertices();
                if occ & bit != 0 {
                    return Err(E::Occupied);
                }
                if topo.vertex_adj_mask[v as usize] & occ != 0 {
                    return Err(E::TooCloseToBuilding);
                }
                if ps.settlements_left == 0 {
                    return Err(E::NoPiecesLeft);
                }
                let ground = if setup {
                    self.board.setup_vertices
                } else {
                    self.board.land_vertices
                };
                if ground & bit == 0 {
                    return Err(E::WrongGround);
                }
                if !setup {
                    if !ps.hand.contains(&cost::SETTLEMENT) {
                        return Err(E::InsufficientResources);
                    }
                    let route = self.roads[player as usize] | self.ships[player as usize];
                    if topo.vertex_edge_mask[v as usize] & route == 0 {
                        return Err(E::NotConnected);
                    }
                }
                Ok(())
            }

            Action::BuildShip(e) => {
                if !self.board.layout.has_ships() {
                    return Err(E::WrongGround);
                }
                if e as usize >= topo.num_edges {
                    return Err(E::InvalidEdge);
                }
                let bit = 1u128 << e;
                if self.occupied_edges() & bit != 0 {
                    return Err(E::Occupied);
                }
                if self.board.sea_edges & bit == 0 {
                    return Err(E::WrongGround);
                }
                if self.pirate_edges() & bit != 0 {
                    return Err(E::PirateBlocks);
                }
                match self.phase {
                    Phase::SetupRoad { vertex } => {
                        if topo.vertex_edge_mask[vertex as usize] & bit == 0 {
                            return Err(E::NotConnected);
                        }
                        Ok(())
                    }
                    Phase::Main | Phase::RoadBuilding { .. } | Phase::SpecialBuild { .. } => {
                        if ps.ships_left == 0 {
                            return Err(E::NoPiecesLeft);
                        }
                        let paid = !matches!(self.phase, Phase::RoadBuilding { .. });
                        if paid && !ps.hand.contains(&cost::SHIP) {
                            return Err(E::InsufficientResources);
                        }
                        if self.ship_spots(player) & bit == 0 {
                            return Err(E::NotConnected);
                        }
                        Ok(())
                    }
                    _ => Err(E::WrongPhase),
                }
            }

            Action::MoveShip { from, to } => {
                self.require_phase(Phase::Main)?;
                if self.ship_moved {
                    return Err(E::ShipAlreadyMoved);
                }
                if from as usize >= topo.num_edges || to as usize >= topo.num_edges {
                    return Err(E::InvalidEdge);
                }
                let (from_bit, to_bit) = (1u128 << from, 1u128 << to);
                if self.pirate_edges() & (from_bit | to_bit) != 0 {
                    return Err(E::PirateBlocks);
                }
                if self.movable_ships(player) & from_bit == 0 || from == to {
                    return Err(E::ShipCannotMove);
                }
                if self.occupied_edges() & to_bit != 0 {
                    return Err(E::Occupied);
                }
                if self.board.sea_edges & to_bit == 0 {
                    return Err(E::WrongGround);
                }
                if self.ship_moves(player, from) & to_bit == 0 {
                    return Err(E::NotConnected);
                }
                Ok(())
            }

            Action::BuildCity(v) => {
                if !(special || self.phase == Phase::Main) {
                    return Err(E::WrongPhase);
                }
                if v as usize >= topo.num_vertices {
                    return Err(E::InvalidVertex);
                }
                if self.settlements[player as usize] & (1u128 << v) == 0 {
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
                if !(special || self.phase == Phase::Main) {
                    return Err(E::WrongPhase);
                }
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
                if !self.can_build_free_route(player) {
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
                if tile as usize >= topo.num_tiles {
                    return Err(E::InvalidTile);
                }
                // A sea tile moves the pirate, a land tile the robber.
                if tile == self.robber || tile == self.pirate {
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

            Action::EndTurn => {
                if special {
                    Ok(())
                } else {
                    self.require_phase(Phase::Main)
                }
            }

            Action::Discard(_) | Action::RespondTrade { .. } | Action::ChooseGold(_) => {
                unreachable!("handled above")
            }
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

    /// Somewhere to place a free road or ship (Road Building).
    fn can_build_free_route(&self, player: u8) -> bool {
        let ps = &self.players[player as usize];
        (ps.roads_left > 0 && self.road_spots(player) != 0)
            || (ps.ships_left > 0 && self.ship_spots(player) != 0)
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
                    self.phase = if self.produce(sum, sink) {
                        Phase::Gold { vertex: None }
                    } else {
                        Phase::Main
                    };
                }
            }

            Action::BuildRoad(e) => {
                let free = matches!(
                    self.phase,
                    Phase::SetupRoad { .. } | Phase::RoadBuilding { .. }
                );
                if !free {
                    self.pay(p, cost::ROAD);
                }
                self.roads[p] |= 1u128 << e;
                self.players[p].roads_left -= 1;
                sink.emit(Event::RoadBuilt { player, edge: e });
                self.update_longest_road_after_build(player, sink);
                self.after_route_piece(player);
            }

            Action::BuildShip(e) => {
                let free = matches!(
                    self.phase,
                    Phase::SetupRoad { .. } | Phase::RoadBuilding { .. }
                );
                if !free {
                    self.pay(p, cost::SHIP);
                }
                let bit = 1u128 << e;
                self.ships[p] |= bit;
                self.players[p].ships_left -= 1;
                if !self.is_setup() && player == self.current {
                    self.new_ships |= bit;
                }
                sink.emit(Event::ShipBuilt { player, edge: e });
                self.update_longest_road_after_build(player, sink);
                self.after_route_piece(player);
            }

            Action::MoveShip { from, to } => {
                self.ships[p] = (self.ships[p] & !(1u128 << from)) | (1u128 << to);
                self.ship_moved = true;
                sink.emit(Event::ShipMoved { player, from, to });
                let len = self.compute_longest_road(player);
                if len != self.players[p].longest_road {
                    self.players[p].longest_road = len;
                    self.reassign_longest_road(sink);
                }
            }

            Action::ChooseGold(set) => {
                self.bank -= set;
                self.players[p].hand += set;
                self.pending_gold[p] = 0;
                sink.emit(Event::GoldChosen {
                    player,
                    resources: set,
                });
                if let Phase::Gold { vertex } = self.phase
                    && self.pending_gold.iter().all(|&g| g == 0)
                {
                    self.phase = match vertex {
                        Some(vertex) => Phase::SetupRoad { vertex },
                        None => Phase::Main,
                    };
                }
            }

            Action::BuildSettlement(v) => {
                let setup = self.phase == Phase::SetupSettlement;
                if !setup {
                    self.pay(p, cost::SETTLEMENT);
                }
                self.settlements[p] |= 1u128 << v;
                self.players[p].settlements_left -= 1;
                self.players[p].harbors |= self.board.vertex_harbor[v as usize];
                sink.emit(Event::SettlementBuilt { player, vertex: v });
                let island = self.board.vertex_island[v as usize];
                if setup {
                    self.players[p].home_islands |= 1 << island;
                    let mut gold = 0;
                    if self.is_second_setup_round() {
                        let mut got = ResourceSet::EMPTY;
                        for &t in &self.topo().vertex_tiles[v as usize] {
                            if t == NONE {
                                break;
                            }
                            match self.board.terrain[t as usize] {
                                crate::Terrain::Gold => gold += 1,
                                terrain => {
                                    if let Some(r) = terrain.resource() {
                                        got[r] += 1;
                                    }
                                }
                            }
                        }
                        self.bank -= got;
                        self.players[p].hand += got;
                        sink.emit(Event::StartingResources {
                            player,
                            resources: got,
                        });
                    }
                    self.phase = if gold > 0 {
                        self.pending_gold[p] = gold;
                        let mut owed = [0; MAX_PLAYERS];
                        owed[p] = gold;
                        sink.emit(Event::GoldOwed { owed });
                        Phase::Gold { vertex: Some(v) }
                    } else {
                        Phase::SetupRoad { vertex: v }
                    };
                } else {
                    let bonus = self.board.layout.island_bonus();
                    let bit = 1u8 << island;
                    let ps = &mut self.players[p];
                    if bonus > 0 && island != 0 && (ps.home_islands | ps.islands) & bit == 0 {
                        ps.islands |= bit;
                        sink.emit(Event::IslandSettled {
                            player,
                            island,
                            points: bonus,
                        });
                    }
                    self.update_longest_road_after_break(v, player, sink);
                    // Roads and ships join at one's own settlement, which
                    // can lengthen the builder's own route.
                    let at_v = self.topo().vertex_edge_mask[v as usize];
                    if self.roads[p] & at_v != 0 && self.ships[p] & at_v != 0 {
                        self.update_longest_road_after_build(player, sink);
                    }
                }
            }

            Action::BuildCity(v) => {
                self.pay(p, cost::CITY);
                let bit = 1u128 << v;
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
                // Cards bought while building out of turn are playable from
                // the buyer's next turn.
                if card == DevCard::VictoryPoint || matches!(self.phase, Phase::SpecialBuild { .. })
                {
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
                let ps = &self.players[p];
                let ships = if self.board.layout.has_ships() {
                    ps.ships_left
                } else {
                    0
                };
                self.phase = Phase::RoadBuilding {
                    remaining: (ps.roads_left + ships).min(2),
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
                if self.board.is_sea(tile) {
                    self.pirate = tile;
                    sink.emit(Event::PirateMoved { player, tile });
                } else {
                    self.robber = tile;
                    sink.emit(Event::RobberMoved { player, tile });
                }
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
                let n = self.num_players();
                if let Phase::SpecialBuild { builder } = self.phase {
                    let next = (builder as usize + 1) % n;
                    if next == self.current as usize {
                        self.start_next_turn();
                    } else {
                        self.phase = Phase::SpecialBuild {
                            builder: next as u8,
                        };
                    }
                    sink.emit(Event::SpecialBuildPassed { player });
                } else {
                    let ps = &mut self.players[p];
                    for c in 0..5 {
                        ps.dev_cards[c] += std::mem::take(&mut ps.new_dev_cards[c]);
                    }
                    self.dev_card_played = false;
                    self.ship_moved = false;
                    self.new_ships = 0;
                    self.dice = None;
                    self.turn += 1;
                    sink.emit(Event::TurnEnded { player });
                    if self.board.layout.has_special_build() {
                        self.phase = Phase::SpecialBuild {
                            builder: ((p + 1) % n) as u8,
                        };
                    } else {
                        self.start_next_turn();
                    }
                }
            }
        }

        // A player can only win on their own turn; this also covers points
        // gained during an opponent's turn, which count once theirs begins.
        if self.phase != Phase::GameOver
            && !self.is_setup()
            && !matches!(self.phase, Phase::SpecialBuild { .. })
        {
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
        matches!(
            self.phase,
            Phase::SetupSettlement | Phase::SetupRoad { .. } | Phase::Gold { vertex: Some(_) }
        )
    }

    /// After a road or ship: setup moves on, Road Building counts down.
    fn after_route_piece(&mut self, player: u8) {
        match self.phase {
            Phase::SetupRoad { .. } => self.advance_setup(),
            Phase::RoadBuilding { remaining } => {
                let remaining = remaining - 1;
                self.phase = if remaining == 0 || !self.can_build_free_route(player) {
                    self.after_interrupt()
                } else {
                    Phase::RoadBuilding { remaining }
                };
            }
            _ => {}
        }
    }

    /// Passes the turn to the next seat.
    #[inline]
    fn start_next_turn(&mut self) {
        self.current = ((self.current as usize + 1) % self.num_players()) as u8;
        self.phase = Phase::PreRoll;
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
    /// owed that resource, who then receives what is left. Gold fields owe
    /// picks instead (see [`Phase::Gold`]); returns whether any are owed.
    fn produce<S: EventSink + ?Sized>(&mut self, roll: u8, sink: &mut S) -> bool {
        let n = self.num_players();
        let mut owed = [ResourceSet::EMPTY; MAX_PLAYERS];
        let mut gold = [0u8; MAX_PLAYERS];
        for &t in &self.board.tiles_by_number[roll as usize] {
            if t == NONE || t == self.robber {
                continue;
            }
            let tv = self.topo().tile_vertex_mask[t as usize];
            let terrain = self.board.terrain[t as usize];
            for q in 0..n {
                let got = (self.settlements[q] & tv).count_ones() as u8
                    + 2 * (self.cities[q] & tv).count_ones() as u8;
                match terrain.resource() {
                    Some(r) => owed[q][r] += got,
                    None if terrain == crate::Terrain::Gold => gold[q] += got,
                    None => {}
                }
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
        if gold.iter().all(|&g| g == 0) {
            return false;
        }
        self.pending_gold = gold;
        sink.emit(Event::GoldOwed { owed: gold });
        true
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
        let edges_at_v = self.topo().vertex_edge_mask[v as usize];
        let mut changed = false;
        for q in 0..self.num_players() as u8 {
            let route = self.roads[q as usize] | self.ships[q as usize];
            if q != builder && (route & edges_at_v).count_ones() >= 2 {
                let len = self.compute_longest_road(q);
                if len != self.players[q as usize].longest_road {
                    self.players[q as usize].longest_road = len;
                    changed = true;
                }
            }
        }
        if changed {
            self.reassign_longest_road(sink);
        }
    }

    /// After routes got shorter: the holder keeps the card while still
    /// (co-)longest with 5+; otherwise it goes to the single longest route
    /// of 5+, or is set aside on a tie.
    fn reassign_longest_road<S: EventSink + ?Sized>(&mut self, sink: &mut S) {
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
