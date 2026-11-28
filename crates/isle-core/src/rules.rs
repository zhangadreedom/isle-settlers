//! Rule validation (`check`) and state transitions (`execute`).

use crate::action::{Action, Event, EventSink, RuleError, TradeGood};
use crate::bitset::BitSet;
use crate::resources::{CityTrack, Commodity, CommoditySet, Resource, ResourceSet, cost};
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
                if self.phase != Phase::Discard || self.config.scenario.is_citadels() {
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
            Action::DiscardCitadel {
                resources,
                commodities,
            } => {
                if self.phase != Phase::Discard || !self.config.scenario.is_citadels() {
                    return Err(E::WrongPhase);
                }
                let owed = self.pending_discard[player as usize];
                if owed == 0 {
                    return Err(E::NothingToDiscard);
                }
                if resources.total() + commodities.total() != owed as u32 {
                    return Err(E::WrongDiscardAmount);
                }
                return if ps.hand.contains(&resources) && ps.commodities.contains(&commodities) {
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
        let paired = matches!(self.phase, Phase::PairedTurn { .. });
        let main_or_paired = self.phase == Phase::Main || paired;

        match action {
            Action::RollDice => self.require_phase(Phase::PreRoll),

            Action::BuildRoad(e) => {
                if e as usize >= topo.num_edges {
                    return Err(E::InvalidEdge);
                }
                let bit = BitSet::bit(e);
                if !(self.occupied_edges() & bit).is_empty() {
                    return Err(E::Occupied);
                }
                if (self.board.land_edges & !self.board.river_edges & bit).is_empty() {
                    return Err(E::WrongGround);
                }
                match self.phase {
                    Phase::SetupRoad { vertex } => {
                        if (topo.vertex_edge_mask[vertex as usize] & bit).is_empty() {
                            return Err(E::NotConnected);
                        }
                        Ok(())
                    }
                    Phase::Main
                    | Phase::RoadBuilding { .. }
                    | Phase::SpecialBuild { .. }
                    | Phase::PairedTurn { .. } => {
                        if ps.roads_left == 0 {
                            return Err(E::NoPiecesLeft);
                        }
                        let paid = !matches!(self.phase, Phase::RoadBuilding { .. });
                        if paid && !ps.hand.contains(&cost::ROAD) {
                            return Err(E::InsufficientResources);
                        }
                        if (self.road_spots(player) & bit).is_empty() {
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
                    Phase::Main | Phase::SpecialBuild { .. } | Phase::PairedTurn { .. } => false,
                    _ => return Err(E::WrongPhase),
                };
                if v as usize >= topo.num_vertices {
                    return Err(E::InvalidVertex);
                }
                let bit = BitSet::bit(v);
                let occ = self.occupied_vertices();
                if !(occ & bit).is_empty() || self.knight_owner[v as usize] != NONE {
                    return Err(E::Occupied);
                }
                if !(topo.vertex_adj_mask[v as usize] & occ).is_empty() {
                    return Err(E::TooCloseToBuilding);
                }
                let setup_city =
                    setup && self.config.scenario.is_citadels() && self.is_second_setup_round();
                if setup_city {
                    if ps.cities_left == 0 {
                        return Err(E::NoPiecesLeft);
                    }
                } else if ps.settlements_left == 0 {
                    return Err(E::NoPiecesLeft);
                }
                let ground = if setup {
                    self.board.setup_vertices
                } else {
                    self.board.land_vertices
                };
                if (ground & bit).is_empty() {
                    return Err(E::WrongGround);
                }
                if !setup {
                    if !ps.hand.contains(&cost::SETTLEMENT) {
                        return Err(E::InsufficientResources);
                    }
                    let route = self.roads[player as usize]
                        | self.ships[player as usize]
                        | self.bridges[player as usize];
                    if (topo.vertex_edge_mask[v as usize] & route).is_empty() {
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
                let bit = BitSet::bit(e);
                if !(self.occupied_edges() & bit).is_empty() {
                    return Err(E::Occupied);
                }
                if (self.board.sea_edges & bit).is_empty() {
                    return Err(E::WrongGround);
                }
                if !(self.pirate_edges() & bit).is_empty() {
                    return Err(E::PirateBlocks);
                }
                match self.phase {
                    Phase::SetupRoad { vertex } => {
                        if (topo.vertex_edge_mask[vertex as usize] & bit).is_empty() {
                            return Err(E::NotConnected);
                        }
                        Ok(())
                    }
                    Phase::Main
                    | Phase::RoadBuilding { .. }
                    | Phase::SpecialBuild { .. }
                    | Phase::PairedTurn { .. } => {
                        if ps.ships_left == 0 {
                            return Err(E::NoPiecesLeft);
                        }
                        let paid = !matches!(self.phase, Phase::RoadBuilding { .. });
                        if paid && !ps.hand.contains(&cost::SHIP) {
                            return Err(E::InsufficientResources);
                        }
                        if (self.ship_spots(player) & bit).is_empty() {
                            return Err(E::NotConnected);
                        }
                        Ok(())
                    }
                    _ => Err(E::WrongPhase),
                }
            }

            Action::MoveShip { from, to } => {
                if !main_or_paired {
                    return Err(E::WrongPhase);
                }
                if self.ship_moved {
                    return Err(E::ShipAlreadyMoved);
                }
                if from as usize >= topo.num_edges || to as usize >= topo.num_edges {
                    return Err(E::InvalidEdge);
                }
                let (from_bit, to_bit) = (BitSet::bit(from), BitSet::bit(to));
                if !(self.pirate_edges() & (from_bit | to_bit)).is_empty() {
                    return Err(E::PirateBlocks);
                }
                if (self.movable_ships(player) & from_bit).is_empty() || from == to {
                    return Err(E::ShipCannotMove);
                }
                if !(self.occupied_edges() & to_bit).is_empty() {
                    return Err(E::Occupied);
                }
                if (self.board.sea_edges & to_bit).is_empty() {
                    return Err(E::WrongGround);
                }
                if (self.ship_moves(player, from) & to_bit).is_empty() {
                    return Err(E::NotConnected);
                }
                Ok(())
            }

            Action::BuildCity(v) => {
                if !(special || main_or_paired) {
                    return Err(E::WrongPhase);
                }
                if v as usize >= topo.num_vertices {
                    return Err(E::InvalidVertex);
                }
                if !self.settlements[player as usize].contains(v) {
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
                if !(special || main_or_paired) {
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
                self.check_robber_dest(player, tile, victim)
            }

            Action::MaritimeTrade { give, get } => {
                if !main_or_paired {
                    return Err(E::WrongPhase);
                }
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

            Action::MaritimeTradeGood { give, get } => {
                if !self.config.scenario.is_citadels() || !main_or_paired {
                    return Err(E::WrongPhase);
                }
                if give == get
                    || matches!(
                        (give, get),
                        (TradeGood::Resource(_), TradeGood::Resource(_))
                    )
                {
                    return Err(E::InvalidTrade);
                }
                match give {
                    TradeGood::Resource(r) => {
                        if ps.hand[r] < self.trade_ratio(player, r) {
                            return Err(E::InsufficientResources);
                        }
                    }
                    TradeGood::Commodity(c) => {
                        if ps.commodities[c] < self.commodity_trade_ratio(player) {
                            return Err(E::InsufficientResources);
                        }
                    }
                }
                match get {
                    TradeGood::Resource(r) => {
                        if self.bank[r] == 0 {
                            return Err(E::BankShort);
                        }
                    }
                    TradeGood::Commodity(c) => {
                        if self.commodity_bank[c] == 0 {
                            return Err(E::BankShort);
                        }
                    }
                }
                Ok(())
            }

            Action::UpgradeCity { track, metropolis } => {
                if !self.config.scenario.is_citadels() || !main_or_paired {
                    return Err(E::WrongPhase);
                }
                if self.cities[player as usize].is_empty() {
                    return Err(E::NoCityForUpgrade);
                }
                let cur = ps.city_progress[track.index()];
                if cur >= 5 {
                    return Err(E::NoPiecesLeft);
                }
                if ps.commodities[track.commodity()] < cur + 1 {
                    return Err(E::InsufficientResources);
                }
                if self.upgrade_claims_metropolis(player, track) {
                    match metropolis {
                        Some(v)
                            if (v as usize) < topo.num_vertices
                                && self.non_metropolis_cities(player).contains(v) =>
                        {
                            Ok(())
                        }
                        _ => Err(E::InvalidMetropolis),
                    }
                } else if metropolis.is_none() {
                    Ok(())
                } else {
                    Err(E::InvalidMetropolis)
                }
            }

            Action::RecruitKnight(v) => {
                if !self.config.scenario.is_citadels() || !main_or_paired {
                    return Err(E::WrongPhase);
                }
                if v as usize >= topo.num_vertices {
                    return Err(E::InvalidVertex);
                }
                if ps.knights_left[0] == 0 {
                    return Err(E::NoPiecesLeft);
                }
                if !ps.hand.contains(&cost::KNIGHT) {
                    return Err(E::InsufficientResources);
                }
                if !self.knight_recruit_spots(player).contains(v) {
                    return Err(E::NotConnected);
                }
                Ok(())
            }

            Action::ActivateKnight(v) => {
                if !self.config.scenario.is_citadels() || !main_or_paired {
                    return Err(E::WrongPhase);
                }
                if v as usize >= topo.num_vertices {
                    return Err(E::InvalidVertex);
                }
                if self.knight_owner[v as usize] != player {
                    return Err(E::NotYourKnight);
                }
                if self.knight_active.contains(v) {
                    return Err(E::KnightCannotAct);
                }
                if !ps.hand.contains(&cost::ACTIVATE_KNIGHT) {
                    return Err(E::InsufficientResources);
                }
                Ok(())
            }

            Action::PromoteKnight(v) => {
                if !self.config.scenario.is_citadels() || !main_or_paired {
                    return Err(E::WrongPhase);
                }
                if v as usize >= topo.num_vertices {
                    return Err(E::InvalidVertex);
                }
                if self.knight_owner[v as usize] != player {
                    return Err(E::NotYourKnight);
                }
                if self.knight_promoted.contains(v) {
                    return Err(E::KnightCannotAct);
                }
                let lvl = self.knight_level[v as usize];
                let max_lvl = if ps.city_progress[CityTrack::Politics.index()] >= 3 {
                    3
                } else {
                    2
                };
                if lvl == 0 || lvl >= max_lvl {
                    return Err(E::KnightCannotAct);
                }
                if ps.knights_left[lvl as usize] == 0 {
                    return Err(E::NoPiecesLeft);
                }
                if !ps.hand.contains(&cost::KNIGHT) {
                    return Err(E::InsufficientResources);
                }
                Ok(())
            }

            Action::MoveKnight { from, to } => {
                if !self.config.scenario.is_citadels() || !main_or_paired {
                    return Err(E::WrongPhase);
                }
                if from as usize >= topo.num_vertices || to as usize >= topo.num_vertices {
                    return Err(E::InvalidVertex);
                }
                if self.knight_owner[from as usize] != player {
                    return Err(E::NotYourKnight);
                }
                if !self.knight_active.contains(from) {
                    return Err(E::KnightNotActive);
                }
                if self.knight_fresh.contains(from) {
                    return Err(E::KnightCannotAct);
                }
                if !self.knight_move_targets(player, from).contains(to) {
                    return Err(E::NotConnected);
                }
                Ok(())
            }

            Action::ChaseRobber {
                vertex,
                tile,
                victim,
            } => {
                if !self.config.scenario.is_citadels() || !main_or_paired {
                    return Err(E::WrongPhase);
                }
                if vertex as usize >= topo.num_vertices {
                    return Err(E::InvalidVertex);
                }
                if self.knight_owner[vertex as usize] != player {
                    return Err(E::NotYourKnight);
                }
                if !self.knight_active.contains(vertex) {
                    return Err(E::KnightNotActive);
                }
                if self.knight_fresh.contains(vertex) {
                    return Err(E::KnightCannotAct);
                }
                self.check_robber_dest(player, tile, victim)?;
                let near = if self.board.is_sea(tile) {
                    self.pirate != NONE
                        && topo.tile_vertex_mask[self.pirate as usize].contains(vertex)
                } else {
                    topo.tile_vertex_mask[self.robber as usize].contains(vertex)
                };
                if !near {
                    return Err(E::KnightCannotAct);
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
                if special || main_or_paired {
                    Ok(())
                } else {
                    Err(E::WrongPhase)
                }
            }

            Action::PassOldBoot(target) => {
                if !self.can_use_fish() {
                    return Err(E::WrongPhase);
                }
                if self.old_boot_owner != Some(player) {
                    return Err(E::NoSuchCard);
                }
                if target as usize >= self.num_players() || target == player {
                    return Err(E::InvalidPlayer);
                }
                if self.public_victory_points(target) < self.public_victory_points(player) {
                    return Err(E::InvalidVictim);
                }
                Ok(())
            }

            Action::FishDriveOffRobber => {
                if !self.can_use_fish() {
                    return Err(E::WrongPhase);
                }
                if ps.fish < 2 {
                    return Err(E::InsufficientResources);
                }
                if self.board.desert == NONE || self.robber == self.board.desert {
                    return Err(E::RobberMustMove);
                }
                Ok(())
            }

            Action::FishSteal(victim) => {
                if !self.can_use_fish() {
                    return Err(E::WrongPhase);
                }
                if ps.fish < 3 {
                    return Err(E::InsufficientResources);
                }
                if victim as usize >= self.num_players()
                    || victim == player
                    || self.players[victim as usize].total_cards() == 0
                {
                    return Err(E::InvalidVictim);
                }
                Ok(())
            }

            Action::FishTakeResource(r) => {
                if !self.can_use_fish() {
                    return Err(E::WrongPhase);
                }
                if ps.fish < 4 {
                    return Err(E::InsufficientResources);
                }
                if self.bank[r] == 0 {
                    return Err(E::BankShort);
                }
                Ok(())
            }

            Action::FishBuildRoad(e) => {
                if !self.can_use_fish() {
                    return Err(E::WrongPhase);
                }
                if ps.fish < 5 {
                    return Err(E::InsufficientResources);
                }
                if ps.roads_left == 0 {
                    return Err(E::NoPiecesLeft);
                }
                if e as usize >= topo.num_edges {
                    return Err(E::InvalidEdge);
                }
                let bit = BitSet::bit(e);
                if !(self.occupied_edges() & bit).is_empty() {
                    return Err(E::Occupied);
                }
                if (self.board.land_edges & !self.board.river_edges & bit).is_empty() {
                    return Err(E::WrongGround);
                }
                if (self.road_spots(player) & bit).is_empty() {
                    return Err(E::NotConnected);
                }
                Ok(())
            }

            Action::FishBuyDevCard => {
                if !self.can_use_fish() {
                    return Err(E::WrongPhase);
                }
                if ps.fish < 7 {
                    return Err(E::InsufficientResources);
                }
                if self.dev_deck_len == 0 {
                    return Err(E::DeckEmpty);
                }
                Ok(())
            }

            Action::PlaceCamel(e) => {
                self.require_phase(Phase::PlaceCamel)?;
                if e as usize >= topo.num_edges {
                    return Err(E::InvalidEdge);
                }
                let bit = BitSet::bit(e);
                if !(self.camels & bit).is_empty() {
                    return Err(E::Occupied);
                }
                if (self.board.land_edges & bit).is_empty() {
                    return Err(E::WrongGround);
                }
                if (self.camel_spots() & bit).is_empty() {
                    return Err(E::NotConnected);
                }
                Ok(())
            }

            Action::BuildBridge(e) => {
                if !self.config.scenario.is_caravans() {
                    return Err(E::WrongPhase);
                }
                if !matches!(
                    self.phase,
                    Phase::Main
                        | Phase::RoadBuilding { .. }
                        | Phase::SpecialBuild { .. }
                        | Phase::PairedTurn { .. }
                ) {
                    return Err(E::WrongPhase);
                }
                if e as usize >= topo.num_edges {
                    return Err(E::InvalidEdge);
                }
                let bit = BitSet::bit(e);
                if !(self.occupied_edges() & bit).is_empty() {
                    return Err(E::Occupied);
                }
                if (self.board.river_edges & bit).is_empty() {
                    return Err(E::WrongGround);
                }
                if ps.bridges_left == 0 {
                    return Err(E::NoPiecesLeft);
                }
                let paid = !matches!(self.phase, Phase::RoadBuilding { .. });
                if paid && !ps.hand.contains(&cost::BRIDGE) {
                    return Err(E::InsufficientResources);
                }
                if (self.bridge_spots(player) & bit).is_empty() {
                    return Err(E::NotConnected);
                }
                Ok(())
            }

            Action::SpendCoinsResource(r) => {
                if !self.config.scenario.uses_coins() || !main_or_paired {
                    return Err(E::WrongPhase);
                }
                if ps.coins < 2 {
                    return Err(E::InsufficientResources);
                }
                if self.bank[r] == 0 {
                    return Err(E::BankShort);
                }
                Ok(())
            }

            Action::RaidPirateLair(tile) => {
                if !self.can_run_explorer_mission() {
                    return Err(E::WrongPhase);
                }
                if ps.explorer_missions[0] >= 3 {
                    return Err(E::NoPiecesLeft);
                }
                if tile as usize >= topo.num_tiles || !self.board.pirate_lairs.contains(tile) {
                    return Err(E::InvalidTile);
                }
                if !self.can_reach_pirate_lair(player, tile) {
                    return Err(E::NotConnected);
                }
                if !ps.hand.contains(&cost::PIRATE_LAIR) {
                    return Err(E::InsufficientResources);
                }
                Ok(())
            }

            Action::HaulFishShoal(tile) => {
                if !self.can_run_explorer_mission() {
                    return Err(E::WrongPhase);
                }
                if ps.explorer_missions[1] >= 3 {
                    return Err(E::NoPiecesLeft);
                }
                if tile as usize >= topo.num_tiles || !self.board.deep_shoals.contains(tile) {
                    return Err(E::InvalidTile);
                }
                if !self.can_reach_deep_shoal(player, tile) {
                    return Err(E::NotConnected);
                }
                if !ps.hand.contains(&cost::FISH_SHOAL) {
                    return Err(E::InsufficientResources);
                }
                Ok(())
            }

            Action::TradeSpice(tile) => {
                if !self.can_run_explorer_mission() {
                    return Err(E::WrongPhase);
                }
                if ps.explorer_missions[2] >= 3 {
                    return Err(E::NoPiecesLeft);
                }
                if tile as usize >= topo.num_tiles || !self.board.spice_islands.contains(tile) {
                    return Err(E::InvalidTile);
                }
                if !self.can_reach_spice_island(player, tile) {
                    return Err(E::NotConnected);
                }
                if !ps.hand.contains(&cost::SPICE_TRADE) {
                    return Err(E::InsufficientResources);
                }
                Ok(())
            }

            Action::Discard(_)
            | Action::DiscardCitadel { .. }
            | Action::RespondTrade { .. }
            | Action::ChooseGold(_) => {
                unreachable!("handled above")
            }
        }
    }

    fn check_robber_dest(
        &self,
        player: u8,
        tile: u8,
        victim: Option<u8>,
    ) -> Result<(), RuleError> {
        let topo = self.topo();
        if tile as usize >= topo.num_tiles
            || self.board.terrain[tile as usize] == crate::Terrain::Fog
            || (self.board.is_sea(tile) && self.pirate == NONE)
        {
            return Err(E::InvalidTile);
        }
        if tile == self.robber || tile == self.pirate {
            return Err(E::RobberMustMove);
        }
        if self.friendly_robber_blocks(tile, player) {
            return Err(E::FriendlyRobber);
        }
        let victims = self.robber_victims(tile, player);
        match victim {
            None if victims == 0 => Ok(()),
            Some(q) if (q as usize) < self.num_players() && victims & (1 << q) != 0 => Ok(()),
            _ => Err(E::InvalidVictim),
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

    /// Somewhere to place a free road, ship or bridge (Road Building).
    fn can_build_free_route(&self, player: u8) -> bool {
        let ps = &self.players[player as usize];
        (ps.roads_left > 0 && !self.road_spots(player).is_empty())
            || (ps.ships_left > 0 && !self.ship_spots(player).is_empty())
            || (self.config.scenario.is_caravans()
                && ps.bridges_left > 0
                && !self.bridge_spots(player).is_empty())
    }

    fn check_dev_card(&self, player: u8, card: DevCard) -> Result<(), RuleError> {
        if !matches!(
            self.phase,
            Phase::PreRoll | Phase::Main | Phase::PairedTurn { .. }
        ) {
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
                let dice = if self.config.event_deck {
                    if self.dice_deck_len <= DICE_DECK_RESHUFFLE_AT {
                        self.dice_deck = CANONICAL_DICE_DECK;
                        self.rng.shuffle(&mut self.dice_deck);
                        self.dice_deck_len = DICE_DECK_SIZE as u8;
                    }
                    self.dice_deck_len -= 1;
                    self.dice_deck[self.dice_deck_len as usize]
                } else {
                    (self.rng.die(), self.rng.die())
                };
                self.dice = Some(dice);
                sink.emit(Event::DiceRolled { player, dice });
                if self.config.scenario.is_citadels() {
                    self.advance_barbarians(sink);
                }
                let sum = dice.0 + dice.1;
                if sum == 7 {
                    let mut any = false;
                    for q in 0..self.num_players() {
                        let n = self.players[q].total_cards();
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
                    let gold_owed = self.produce(sum, sink);
                    if self.config.scenario.is_fishermen() {
                        self.produce_fish(sum, sink);
                    }
                    self.phase = if gold_owed {
                        self.after_gold = Phase::Main;
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
                self.roads[p].insert(e);
                self.players[p].roads_left -= 1;
                sink.emit(Event::RoadBuilt { player, edge: e });
                let gold = self.reveal_fog_at_edge(player, e, sink);
                self.update_longest_road_after_build(player, sink);
                self.after_route_piece(player);
                self.owe_discovered_gold(player, gold, sink);
            }

            Action::BuildShip(e) => {
                let free = matches!(
                    self.phase,
                    Phase::SetupRoad { .. } | Phase::RoadBuilding { .. }
                );
                if !free {
                    self.pay(p, cost::SHIP);
                }
                self.ships[p].insert(e);
                self.players[p].ships_left -= 1;
                if !self.is_setup() && !matches!(self.phase, Phase::SpecialBuild { .. }) {
                    self.new_ships.insert(e);
                }
                sink.emit(Event::ShipBuilt { player, edge: e });
                let gold = self.reveal_fog_at_edge(player, e, sink);
                self.update_longest_road_after_build(player, sink);
                self.after_route_piece(player);
                self.owe_discovered_gold(player, gold, sink);
            }

            Action::MoveShip { from, to } => {
                self.ships[p].remove(from);
                self.ships[p].insert(to);
                self.ship_moved = true;
                sink.emit(Event::ShipMoved { player, from, to });
                let gold = self.reveal_fog_at_edge(player, to, sink);
                let len = self.compute_longest_road(player);
                if len != self.players[p].longest_road {
                    self.players[p].longest_road = len;
                    self.reassign_longest_road(sink);
                }
                self.owe_discovered_gold(player, gold, sink);
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
                        None => std::mem::replace(&mut self.after_gold, Phase::Main),
                    };
                }
            }

            Action::BuildSettlement(v) => {
                let setup = self.phase == Phase::SetupSettlement;
                let setup_city =
                    setup && self.config.scenario.is_citadels() && self.is_second_setup_round();
                if !setup {
                    self.pay(p, cost::SETTLEMENT);
                }
                self.players[p].harbors |= self.board.vertex_harbor[v as usize];
                if setup_city {
                    self.cities[p].insert(v);
                    self.players[p].cities_left -= 1;
                    sink.emit(Event::CityBuilt { player, vertex: v });
                } else {
                    self.settlements[p].insert(v);
                    self.players[p].settlements_left -= 1;
                    sink.emit(Event::SettlementBuilt { player, vertex: v });
                }
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
                        if self.config.scenario.is_caravans() {
                            self.gain_coins(player, 2, sink);
                        }
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
                    if !((self.roads[p] | self.bridges[p]) & at_v).is_empty()
                        && !(self.ships[p] & at_v).is_empty()
                    {
                        self.update_longest_road_after_build(player, sink);
                    }
                    if self.config.scenario.is_caravans() {
                        self.check_winner(sink);
                        if self.winner.is_none() && !self.camel_spots().is_empty() {
                            self.after_camel = self.phase;
                            self.phase = Phase::PlaceCamel;
                        }
                    }
                }
            }

            Action::BuildCity(v) => {
                self.pay(p, cost::CITY);
                self.settlements[p].remove(v);
                self.cities[p].insert(v);
                self.players[p].settlements_left += 1;
                self.players[p].cities_left -= 1;
                sink.emit(Event::CityBuilt { player, vertex: v });
                if self.config.scenario.is_caravans() {
                    self.check_winner(sink);
                    if self.winner.is_none() && !self.camel_spots().is_empty() {
                        self.after_camel = self.phase;
                        self.phase = Phase::PlaceCamel;
                    }
                }
            }

            Action::BuyDevCard => {
                self.pay(p, cost::DEV_CARD);
                self.dev_deck_len -= 1;
                let card = self.dev_deck[self.dev_deck_len as usize];
                // Cards bought while building out of turn are playable from
                // the buyer's next primary turn.
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
                let bridges = if self.config.scenario.is_caravans() {
                    ps.bridges_left
                } else {
                    0
                };
                self.phase = Phase::RoadBuilding {
                    remaining: ps.roads_left.saturating_add(ships).saturating_add(bridges).min(2),
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
                self.move_robber_and_steal(player, tile, victim, sink);
                self.phase = self.after_interrupt();
            }

            Action::ChaseRobber {
                vertex,
                tile,
                victim,
            } => {
                self.knight_active.remove(vertex);
                self.move_robber_and_steal(player, tile, victim, sink);
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

            Action::DiscardCitadel {
                resources,
                commodities,
            } => {
                if !resources.is_empty() {
                    self.players[p].hand -= resources;
                    self.bank += resources;
                    sink.emit(Event::Discarded {
                        player,
                        resources,
                    });
                }
                if !commodities.is_empty() {
                    self.players[p].commodities -= commodities;
                    self.commodity_bank += commodities;
                    sink.emit(Event::CommoditiesDiscarded {
                        player,
                        commodities,
                    });
                }
                self.pending_discard[p] = 0;
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

            Action::MaritimeTradeGood { give, get } => {
                let ratio = match give {
                    TradeGood::Resource(r) => {
                        let ratio = self.trade_ratio(player, r);
                        self.players[p].hand[r] -= ratio;
                        self.bank[r] += ratio;
                        ratio
                    }
                    TradeGood::Commodity(c) => {
                        let ratio = self.commodity_trade_ratio(player);
                        self.players[p].commodities[c] -= ratio;
                        self.commodity_bank[c] += ratio;
                        ratio
                    }
                };
                match get {
                    TradeGood::Resource(r) => {
                        self.bank[r] -= 1;
                        self.players[p].hand[r] += 1;
                    }
                    TradeGood::Commodity(c) => {
                        self.commodity_bank[c] -= 1;
                        self.players[p].commodities[c] += 1;
                    }
                }
                sink.emit(Event::GoodTraded {
                    player,
                    give,
                    ratio,
                    get,
                });
            }

            Action::UpgradeCity { track, metropolis } => {
                let idx = track.index();
                let next = self.players[p].city_progress[idx] + 1;
                let comm = track.commodity();
                self.players[p].commodities[comm] -= next;
                self.commodity_bank[comm] += next;
                self.players[p].city_progress[idx] = next;
                if let Some(mv) = metropolis {
                    self.metropolises[idx] = mv;
                    self.metropolis_owners[idx] = Some(player);
                }
                sink.emit(Event::CityUpgraded {
                    player,
                    track,
                    level: next,
                    metropolis,
                });
            }

            Action::RecruitKnight(v) => {
                self.pay(p, cost::KNIGHT);
                self.players[p].knights_left[0] -= 1;
                self.knight_owner[v as usize] = player;
                self.knight_level[v as usize] = 1;
                self.knight_active.remove(v);
                self.knight_fresh.remove(v);
                sink.emit(Event::KnightRecruited { player, vertex: v });
                self.recompute_all_longest_roads(sink);
            }

            Action::ActivateKnight(v) => {
                self.pay(p, cost::ACTIVATE_KNIGHT);
                self.knight_active.insert(v);
                self.knight_fresh.insert(v);
                sink.emit(Event::KnightActivated { player, vertex: v });
            }

            Action::PromoteKnight(v) => {
                self.pay(p, cost::KNIGHT);
                let lvl = self.knight_level[v as usize];
                self.players[p].knights_left[(lvl - 1) as usize] += 1;
                self.players[p].knights_left[lvl as usize] -= 1;
                self.knight_level[v as usize] = lvl + 1;
                self.knight_promoted.insert(v);
                sink.emit(Event::KnightPromoted {
                    player,
                    vertex: v,
                    level: lvl + 1,
                });
            }

            Action::MoveKnight { from, to } => {
                let lvl = self.knight_level[from as usize];
                self.knight_owner[from as usize] = NONE;
                self.knight_level[from as usize] = 0;
                self.knight_active.remove(from);
                self.knight_fresh.remove(from);
                self.knight_promoted.remove(from);

                let displaced = if self.knight_owner[to as usize] != NONE {
                    let opp = self.knight_owner[to as usize];
                    let opp_lvl = self.knight_level[to as usize];
                    let opp_active = self.knight_active.contains(to);
                    self.knight_active.remove(to);
                    self.knight_fresh.remove(to);
                    self.knight_promoted.remove(to);
                    let retreat = self.knight_retreat_spot(opp, to, from);
                    if let Some(rv) = retreat {
                        self.knight_owner[rv as usize] = opp;
                        self.knight_level[rv as usize] = opp_lvl;
                        if opp_active {
                            self.knight_active.insert(rv);
                        }
                    } else {
                        self.players[opp as usize].knights_left[(opp_lvl - 1) as usize] += 1;
                    }
                    Some((opp, retreat))
                } else {
                    None
                };

                self.knight_owner[to as usize] = player;
                self.knight_level[to as usize] = lvl;
                sink.emit(Event::KnightMoved {
                    player,
                    from,
                    to,
                    displaced,
                });
                self.recompute_all_longest_roads(sink);
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

            Action::PassOldBoot(target) => {
                self.old_boot_owner = Some(target);
                sink.emit(Event::OldBootPassed {
                    from: player,
                    to: target,
                });
            }

            Action::FishDriveOffRobber => {
                let desert = self.board.desert;
                self.players[p].fish -= 2;
                self.robber = desert;
                sink.emit(Event::FishSpent { player, cost: 2 });
                sink.emit(Event::RobberMoved {
                    player,
                    tile: desert,
                });
            }

            Action::FishSteal(victim) => {
                self.players[p].fish -= 3;
                sink.emit(Event::FishSpent { player, cost: 3 });
                self.steal_from(player, victim, sink);
            }

            Action::FishTakeResource(r) => {
                self.players[p].fish -= 4;
                self.bank[r] -= 1;
                self.players[p].hand[r] += 1;
                sink.emit(Event::FishSpent { player, cost: 4 });
                sink.emit(Event::FishResourceTaken {
                    player,
                    resource: r,
                });
            }

            Action::FishBuildRoad(e) => {
                self.players[p].fish -= 5;
                sink.emit(Event::FishSpent { player, cost: 5 });
                self.roads[p].insert(e);
                self.players[p].roads_left -= 1;
                sink.emit(Event::RoadBuilt { player, edge: e });
                let gold = self.reveal_fog_at_edge(player, e, sink);
                self.update_longest_road_after_build(player, sink);
                self.owe_discovered_gold(player, gold, sink);
            }

            Action::FishBuyDevCard => {
                self.players[p].fish -= 7;
                sink.emit(Event::FishSpent { player, cost: 7 });
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

            Action::PlaceCamel(edge) => {
                self.camels.insert(edge);
                sink.emit(Event::CamelPlaced { player, edge });
                for q in 0..self.num_players() as u8 {
                    if (self.roads[q as usize] | self.bridges[q as usize]).contains(edge) {
                        self.update_longest_road_after_build(q, sink);
                    }
                }
                self.phase = std::mem::replace(&mut self.after_camel, Phase::Main);
            }

            Action::BuildBridge(e) => {
                let free = matches!(self.phase, Phase::RoadBuilding { .. });
                if !free {
                    self.pay(p, cost::BRIDGE);
                }
                self.bridges[p].insert(e);
                self.players[p].bridges_left -= 1;
                sink.emit(Event::BridgeBuilt { player, edge: e });
                self.gain_coins(player, 2, sink);
                self.update_longest_road_after_build(player, sink);
                self.after_route_piece(player);
            }

            Action::SpendCoinsResource(r) => {
                self.players[p].coins -= 2;
                self.bank[r] -= 1;
                self.players[p].hand[r] += 1;
                self.reassign_wealthiest();
                sink.emit(Event::CoinsSpent {
                    player,
                    cost: 2,
                    resource: r,
                });
            }

            Action::RaidPirateLair(_) => {
                self.complete_explorer_mission(player, 0, cost::PIRATE_LAIR, sink);
            }

            Action::HaulFishShoal(_) => {
                self.complete_explorer_mission(player, 1, cost::FISH_SHOAL, sink);
            }

            Action::TradeSpice(_) => {
                self.complete_explorer_mission(player, 2, cost::SPICE_TRADE, sink);
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
                } else if matches!(self.phase, Phase::PairedTurn { .. }) {
                    let ps = &mut self.players[p];
                    for c in 0..5 {
                        ps.dev_cards[c] += std::mem::take(&mut ps.new_dev_cards[c]);
                    }
                    self.dev_card_played = false;
                    self.ship_moved = false;
                    self.new_ships = BitSet::EMPTY;
                    self.knight_fresh = BitSet::EMPTY;
                    self.knight_promoted = BitSet::EMPTY;
                    self.in_paired_turn = None;
                    sink.emit(Event::PairedTurnEnded { player });
                    self.start_next_turn();
                } else {
                    let ps = &mut self.players[p];
                    for c in 0..5 {
                        ps.dev_cards[c] += std::mem::take(&mut ps.new_dev_cards[c]);
                    }
                    self.dev_card_played = false;
                    self.ship_moved = false;
                    self.new_ships = BitSet::EMPTY;
                    self.knight_fresh = BitSet::EMPTY;
                    self.knight_promoted = BitSet::EMPTY;
                    self.dice = None;
                    self.turn += 1;
                    sink.emit(Event::TurnEnded { player });
                    if self.board.layout.has_special_build() {
                        if self.config.paired_turns {
                            let partner = ((p + 3) % n) as u8;
                            self.in_paired_turn = Some(partner);
                            self.phase = Phase::PairedTurn { partner };
                        } else {
                            self.phase = Phase::SpecialBuild {
                                builder: ((p + 1) % n) as u8,
                            };
                        }
                    } else {
                        self.start_next_turn();
                    }
                }
            }
        }

        self.check_winner(sink);
    }

    fn check_winner<S: EventSink + ?Sized>(&mut self, sink: &mut S) {
        // A player can only win on their own primary turn; points gained
        // during an opponent's turn or a secondary phase count once theirs begins.
        if self.phase != Phase::GameOver
            && !self.is_setup()
            && !matches!(
                self.phase,
                Phase::SpecialBuild { .. }
                    | Phase::PairedTurn { .. }
                    | Phase::PlaceCamel
            )
            && self.in_paired_turn.is_none()
        {
            let vp = self.victory_points(self.current);
            if vp >= self.target_victory_points(self.current) {
                self.winner = Some(self.current);
                self.phase = Phase::GameOver;
                self.trade = None;
                self.pending_gold = [0; MAX_PLAYERS];
                self.pending_discard = [0; MAX_PLAYERS];
                sink.emit(Event::GameWon {
                    player: self.current,
                    points: vp,
                });
            }
        }
    }

    fn complete_explorer_mission<S: EventSink + ?Sized>(
        &mut self,
        player: u8,
        mission: u8,
        price: ResourceSet,
        sink: &mut S,
    ) {
        let p = player as usize;
        let m = mission as usize;
        self.pay(p, price);
        self.players[p].explorer_missions[m] += 1;
        let step = self.players[p].explorer_missions[m];
        let champion_claimed = if step == 3 && self.explorer_champions[m].is_none() {
            self.explorer_champions[m] = Some(player);
            true
        } else {
            false
        };
        let coins = 2 + u8::from(self.players[p].explorer_missions[2] >= 1);
        sink.emit(Event::ExplorerMission {
            player,
            mission,
            step,
            champion_claimed,
        });
        self.gain_coins(player, coins, sink);
    }

    fn gain_coins<S: EventSink + ?Sized>(&mut self, player: u8, amount: u8, sink: &mut S) {
        let p = player as usize;
        let before = self.players[p].coins;
        let after = before.saturating_add(amount).min(MAX_COINS);
        let gained = after - before;
        self.players[p].coins = after;
        if gained > 0 {
            sink.emit(Event::CoinsGained {
                player,
                amount: gained,
            });
            self.reassign_wealthiest();
        }
    }

    pub fn reassign_wealthiest(&mut self) {
        if !self.config.scenario.is_caravans() {
            self.wealthiest_owner = None;
            return;
        }
        let max = (0..self.num_players())
            .map(|q| self.players[q].coins)
            .max()
            .unwrap_or(0);
        if let Some(h) = self.wealthiest_owner {
            let c = self.players[h as usize].coins;
            if c >= WEALTHIEST_MIN_COINS && c == max {
                return;
            }
        }
        let mut leaders =
            (0..self.num_players() as u8).filter(|&q| self.players[q as usize].coins == max);
        self.wealthiest_owner = match (leaders.next(), leaders.next()) {
            (Some(q), None) if max >= WEALTHIEST_MIN_COINS => Some(q),
            _ => None,
        };
    }

    fn move_robber_and_steal<S: EventSink + ?Sized>(
        &mut self,
        player: u8,
        tile: u8,
        victim: Option<u8>,
        sink: &mut S,
    ) {
        if self.board.is_sea(tile) {
            self.pirate = tile;
            sink.emit(Event::PirateMoved { player, tile });
        } else {
            self.robber = tile;
            sink.emit(Event::RobberMoved { player, tile });
        }
        if let Some(q) = victim {
            self.steal_from(player, q, sink);
        }
    }

    fn steal_from<S: EventSink + ?Sized>(&mut self, player: u8, q: u8, sink: &mut S) {
        let p = player as usize;
        let hand = self.players[q as usize].hand;
        let comm = self.players[q as usize].commodities;
        let h_total = hand.total();
        let idx = self.rng.below(h_total + comm.total());
        if idx < h_total {
            let r = hand.nth_card(idx).expect("victim has resource");
            self.players[q as usize].hand[r] -= 1;
            self.players[p].hand[r] += 1;
            sink.emit(Event::Stolen {
                thief: player,
                victim: q,
                resource: Some(r),
            });
        } else {
            let c = comm.nth_card(idx - h_total).expect("victim has commodity");
            self.players[q as usize].commodities[c] -= 1;
            self.players[p].commodities[c] += 1;
            sink.emit(Event::CommodityStolen {
                thief: player,
                victim: q,
                commodity: Some(c),
            });
        }
    }

    /// Citadels: rolls the barbarian event die (50% black ship) and resolves
    /// a raid if the fleet lands at `BARBARIAN_STEPS`.
    fn advance_barbarians<S: EventSink + ?Sized>(&mut self, sink: &mut S) {
        if self.rng.below(6) >= 3 {
            return;
        }
        self.barbarian_pos += 1;
        sink.emit(Event::BarbarianAdvanced {
            pos: self.barbarian_pos,
        });
        if self.barbarian_pos < BARBARIAN_STEPS {
            return;
        }
        self.barbarian_pos = 0;
        let n = self.num_players();
        let cities_total: u8 = (0..n)
            .map(|q| self.non_metropolis_cities(q as u8).count_ones() as u8)
            .sum();
        let mut strength = [0u8; MAX_PLAYERS];
        for q in 0..n {
            strength[q] = self.active_knight_strength(q as u8);
        }
        self.knight_active = BitSet::EMPTY;
        self.knight_fresh = BitSet::EMPTY;
        if cities_total == 0 {
            return;
        }
        let defense_total: u8 = strength[..n].iter().sum();
        if defense_total >= cities_total {
            let max_s = strength[..n].iter().copied().max().unwrap_or(0);
            let mut winners = (0..n as u8).filter(|&q| strength[q as usize] == max_s);
            let sole = match (winners.next(), winners.next()) {
                (Some(hero), None) if max_s > 0 => Some(hero),
                _ => None,
            };
            if let Some(hero) = sole {
                self.players[hero as usize].defender_points += 1;
            } else if max_s > 0 {
                for q in 0..n {
                    if strength[q] == max_s && self.dev_deck_len > 0 {
                        self.dev_deck_len -= 1;
                        let card = self.dev_deck[self.dev_deck_len as usize];
                        if card == DevCard::VictoryPoint || q != self.current as usize {
                            self.players[q].dev_cards[card.index()] += 1;
                        } else {
                            self.players[q].new_dev_cards[card.index()] += 1;
                        }
                        sink.emit(Event::DevCardBought {
                            player: q as u8,
                            card: Some(card),
                        });
                    }
                }
            }
            sink.emit(Event::BarbariansRepelled {
                cities: cities_total,
                defense: defense_total,
                sole_defender: sole,
            });
        } else {
            let min_s = (0..n)
                .filter(|&q| !self.non_metropolis_cities(q as u8).is_empty())
                .map(|q| strength[q])
                .min();
            if let Some(min_s) = min_s {
                for q in 0..n {
                    let vuln = self.non_metropolis_cities(q as u8);
                    if strength[q] == min_s
                        && let Some(v) = vertices_in(vuln).next()
                    {
                        self.cities[q].remove(v);
                        self.players[q].cities_left += 1;
                        if self.players[q].settlements_left > 0 {
                            self.settlements[q].insert(v);
                            self.players[q].settlements_left -= 1;
                        }
                        sink.emit(Event::CityPillaged {
                            player: q as u8,
                            vertex: v,
                        });
                    }
                }
                self.recompute_all_longest_roads(sink);
            }
        }
    }

    fn recompute_all_longest_roads<S: EventSink + ?Sized>(&mut self, sink: &mut S) {
        let mut changed = false;
        for q in 0..self.num_players() as u8 {
            let len = self.compute_longest_road(q);
            if len != self.players[q as usize].longest_road {
                self.players[q as usize].longest_road = len;
                changed = true;
            }
        }
        if changed {
            self.reassign_longest_road(sink);
        }
    }

    #[inline]
    pub fn is_setup(&self) -> bool {
        matches!(
            self.phase,
            Phase::SetupSettlement | Phase::SetupRoad { .. } | Phase::Gold { vertex: Some(_) }
        ) || (matches!(self.phase, Phase::Gold { .. })
            && matches!(
                self.after_gold,
                Phase::SetupSettlement | Phase::SetupRoad { .. }
            ))
    }

    /// Reveals every unexplored fog tile touching either endpoint of `edge`,
    /// paying 1 resource card per regular land tile discovered and returning
    /// the number of gold fields discovered.
    fn reveal_fog_at_edge<S: EventSink + ?Sized>(
        &mut self,
        player: u8,
        edge: u8,
        sink: &mut S,
    ) -> u8 {
        if self.fog_terrain_len == 0 {
            return 0;
        }
        let topo = self.topo();
        let [a, b] = topo.edge_vertices[edge as usize];
        let mut mask = 0u64;
        for v in [a, b] {
            for &t in &topo.vertex_tiles[v as usize] {
                if t != NONE && self.board.terrain[t as usize] == crate::Terrain::Fog {
                    mask |= 1u64 << t;
                }
            }
        }
        let mut gold = 0u8;
        while mask != 0 {
            let tile = mask.trailing_zeros() as u8;
            mask &= mask - 1;
            self.fog_terrain_len -= 1;
            let terrain = self.fog_terrain[self.fog_terrain_len as usize];
            let number = if terrain.produces() {
                self.fog_numbers_len -= 1;
                self.fog_numbers[self.fog_numbers_len as usize]
            } else {
                0
            };
            self.board.reveal_tile(tile, terrain, number);
            let mut reward = None;
            if let Some(r) = terrain.resource() {
                if self.bank[r] > 0 {
                    self.bank[r] -= 1;
                    self.players[player as usize].hand[r] += 1;
                    reward = Some(r);
                }
            } else if terrain == crate::Terrain::Gold {
                gold += 1;
            }
            sink.emit(Event::TileRevealed {
                player,
                tile,
                terrain,
                number,
                reward,
            });
        }
        gold
    }

    fn owe_discovered_gold<S: EventSink + ?Sized>(
        &mut self,
        player: u8,
        gold: u8,
        sink: &mut S,
    ) {
        if gold == 0 || self.bank.total() == 0 {
            return;
        }
        self.pending_gold[player as usize] = gold;
        let mut owed = [0; MAX_PLAYERS];
        owed[player as usize] = gold;
        sink.emit(Event::GoldOwed { owed });
        self.after_gold = self.phase;
        self.phase = Phase::Gold { vertex: None };
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
        if let Some(partner) = self.in_paired_turn {
            Phase::PairedTurn { partner }
        } else if self.dice.is_some() {
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

    /// Distributes resources (and in Citadels mode, commodities and Science
    /// Lv3 Aqueduct picks) for a dice roll. Returns whether any `pending_gold`
    /// picks are owed.
    fn produce<S: EventSink + ?Sized>(&mut self, roll: u8, sink: &mut S) -> bool {
        let n = self.num_players();
        let citadels = self.config.scenario.is_citadels();
        let mut owed = [ResourceSet::EMPTY; MAX_PLAYERS];
        let mut comm_owed = [CommoditySet::EMPTY; MAX_PLAYERS];
        let mut gold = [0u8; MAX_PLAYERS];
        for &t in &self.board.tiles_by_number[roll as usize] {
            if t == NONE || t == self.robber {
                continue;
            }
            let tv = self.topo().tile_vertex_mask[t as usize];
            let terrain = self.board.terrain[t as usize];
            for q in 0..n {
                let s_cnt = (self.settlements[q] & tv).count_ones() as u8;
                let c_cnt = (self.cities[q] & tv).count_ones() as u8;
                if citadels
                    && c_cnt > 0
                    && let Some(comm) = terrain.commodity()
                    && let Some(r) = terrain.resource()
                {
                    owed[q][r] += s_cnt + c_cnt;
                    comm_owed[q][comm] += c_cnt;
                } else {
                    let got = s_cnt + 2 * c_cnt;
                    match terrain.resource() {
                        Some(r) => owed[q][r] += got,
                        None if terrain == crate::Terrain::Gold => gold[q] += got,
                        None => {}
                    }
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
        if citadels {
            for c in Commodity::ALL {
                let total: u32 = (0..n).map(|q| comm_owed[q][c] as u32).sum();
                if total == 0 || total <= self.commodity_bank[c] as u32 {
                    continue;
                }
                let claimants = (0..n).filter(|&q| comm_owed[q][c] > 0).count();
                for q in 0..n {
                    comm_owed[q][c] = if claimants == 1 {
                        comm_owed[q][c].min(self.commodity_bank[c])
                    } else {
                        0
                    };
                }
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
            if !comm_owed[q].is_empty() {
                self.commodity_bank -= comm_owed[q];
                self.players[q].commodities += comm_owed[q];
                sink.emit(Event::CommoditiesProduced {
                    player: q as u8,
                    commodities: comm_owed[q],
                });
            }
            // Citadels Science Lv3 (Aqueduct): 1 free chosen resource when a
            // non-7 roll produces nothing for this player.
            if citadels
                && self.players[q].city_progress[CityTrack::Science.index()] >= 3
                && owed[q].is_empty()
                && comm_owed[q].is_empty()
                && gold[q] == 0
                && self.bank.total() > 0
            {
                gold[q] = 1;
            }
        }
        if self.config.scenario.is_caravans() {
            let topo = self.topo();
            let mut coins_owed = [0u8; MAX_PLAYERS];
            for &t in &self.board.tiles_by_number[roll as usize] {
                if t == NONE || t == self.robber {
                    continue;
                }
                let terrain = self.board.terrain[t as usize];
                let gives_coin =
                    matches!(terrain, crate::Terrain::Gold | crate::Terrain::Mountains)
                        || !(topo.tile_edge_mask(t) & self.board.river_edges).is_empty();
                if !gives_coin {
                    continue;
                }
                let tv = topo.tile_vertex_mask[t as usize];
                for q in 0..n {
                    let b_cnt = (self.buildings_of(q as u8) & tv).count_ones() as u8;
                    if b_cnt == 0 {
                        continue;
                    }
                    let produced = match terrain.resource() {
                        Some(r) => owed[q][r] > 0,
                        None => terrain == crate::Terrain::Gold && self.bank.total() > 0,
                    };
                    if produced {
                        coins_owed[q] = coins_owed[q].saturating_add(b_cnt);
                    }
                }
            }
            for q in 0..n {
                if coins_owed[q] > 0 {
                    self.gain_coins(q as u8, coins_owed[q], sink);
                }
            }
        }
        if self.config.scenario.is_explorers() {
            for q in 0..n {
                if owed[q].is_empty() && gold[q] == 0 {
                    self.gain_coins(q as u8, 1, sink);
                }
            }
        }
        if gold.iter().all(|&g| g == 0) {
            return false;
        }
        self.pending_gold = gold;
        sink.emit(Event::GoldOwed { owed: gold });
        true
    }

    /// Fishermen: awards fish tokens from coastal fishing grounds matching
    /// `roll` and from the inland trout lake (`board.desert`) on `2` or `12`
    /// when not blocked by the robber, in turn order starting from `current`.
    fn produce_fish<S: EventSink + ?Sized>(&mut self, roll: u8, sink: &mut S) {
        let n = self.num_players();
        let topo = self.topo();
        let fg_count = self.board.fishing_ground_count as usize;
        let lake = if (roll == 2 || roll == 12)
            && self.board.desert != NONE
            && self.robber != self.board.desert
        {
            topo.tile_vertex_mask[self.board.desert as usize]
        } else {
            BitSet::EMPTY
        };
        for offset in 0..n {
            let q = ((self.current as usize + offset) % n) as u8;
            let qi = q as usize;
            let mut gained = 0u8;
            for fg in &self.board.fishing_grounds[..fg_count] {
                if fg.number == roll {
                    for v in topo.edge_vertices[fg.edge as usize] {
                        if self.settlements[qi].contains(v) {
                            gained = gained.saturating_add(1);
                        } else if self.cities[qi].contains(v) {
                            gained = gained.saturating_add(2);
                        }
                    }
                }
            }
            if !lake.is_empty() {
                let s_cnt = (self.settlements[qi] & lake).count_ones() as u8;
                let c_cnt = (self.cities[qi] & lake).count_ones() as u8;
                gained = gained.saturating_add(s_cnt + 2 * c_cnt);
            }
            let before = self.players[qi].fish;
            let after = before.saturating_add(gained).min(MAX_FISH_HAND);
            let actual_added = after - before;
            self.players[qi].fish = after;
            if actual_added > 0 {
                sink.emit(Event::FishCaught {
                    player: q,
                    count: actual_added,
                });
                if self.old_boot_owner.is_none() && self.rng.below(12) == 0 {
                    self.old_boot_owner = Some(q);
                    sink.emit(Event::OldBootDrawn { player: q });
                }
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
        let edges_at_v = self.topo().vertex_edge_mask[v as usize];
        let mut changed = false;
        for q in 0..self.num_players() as u8 {
            let route =
                self.roads[q as usize] | self.ships[q as usize] | self.bridges[q as usize];
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
