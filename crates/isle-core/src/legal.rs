//! Legal move generation.
//!
//! [`GameState::legal_actions`] lists every legal action for a seat except
//! open-ended domestic trade proposals (`ProposeTrade`), whose space is too
//! large to enumerate; bots generate those themselves and validate them with
//! [`GameState::check`].

use crate::action::Action;
use crate::resources::{Resource, ResourceSet, cost};
use crate::state::*;
use crate::topology::*;

impl GameState {
    /// Fills `out` (cleared first) with the legal actions of `player`.
    /// Reusing one buffer keeps generation allocation-free.
    pub fn legal_actions(&self, player: u8, out: &mut Vec<Action>) {
        out.clear();
        if self.phase == Phase::GameOver || player as usize >= self.num_players() {
            return;
        }
        let ps = &self.players[player as usize];

        if player != self.active_player() {
            match self.phase {
                Phase::Discard if self.pending_discard[player as usize] > 0 => {
                    push_discards(ps.hand, self.pending_discard[player as usize], out);
                }
                Phase::Main => {
                    if let Some(offer) = self.trade {
                        if ps.hand.contains(&offer.get) {
                            out.push(Action::RespondTrade { accept: true });
                        }
                        out.push(Action::RespondTrade { accept: false });
                    }
                }
                Phase::Gold { .. } => self.push_gold_choices(player, out),
                _ => {}
            }
            return;
        }

        match self.phase {
            Phase::SetupSettlement => {
                for v in vertices_in(self.setup_spots()) {
                    out.push(Action::BuildSettlement(v));
                }
            }
            Phase::SetupRoad { vertex } => {
                let free = self.topo().vertex_edge_mask[vertex as usize] & !self.occupied_edges();
                for e in edges_in(free & self.board.land_edges) {
                    out.push(Action::BuildRoad(e));
                }
                if self.board.layout.has_ships() {
                    for e in edges_in(free & self.board.sea_edges & !self.pirate_edges()) {
                        out.push(Action::BuildShip(e));
                    }
                }
            }
            Phase::Gold { .. } => self.push_gold_choices(player, out),
            Phase::PreRoll => {
                out.push(Action::RollDice);
                self.push_dev_card_plays(player, out);
            }
            Phase::Discard => {
                if self.pending_discard[player as usize] > 0 {
                    push_discards(ps.hand, self.pending_discard[player as usize], out);
                }
            }
            Phase::MoveRobber => {
                // Land tiles move the robber, sea tiles the pirate.
                for tile in 0..self.topo().num_tiles as u8 {
                    if tile == self.robber || tile == self.pirate {
                        continue;
                    }
                    if self.board.terrain[tile as usize] == crate::Terrain::Fog {
                        continue;
                    }
                    if self.board.is_sea(tile) && self.pirate == NONE {
                        continue;
                    }
                    let victims = self.robber_victims(tile, player);
                    if victims == 0 {
                        out.push(Action::MoveRobber { tile, victim: None });
                    } else {
                        for q in 0..self.num_players() as u8 {
                            if victims & (1 << q) != 0 {
                                out.push(Action::MoveRobber {
                                    tile,
                                    victim: Some(q),
                                });
                            }
                        }
                    }
                }
            }
            Phase::Main => {
                self.push_builds(player, out);
                if !self.ship_moved {
                    for from in edges_in(self.movable_ships(player)) {
                        for to in edges_in(self.ship_moves(player, from)) {
                            out.push(Action::MoveShip { from, to });
                        }
                    }
                }
                self.push_dev_card_plays(player, out);
                for give in Resource::ALL {
                    if ps.hand[give] >= self.trade_ratio(player, give) {
                        for get in Resource::ALL {
                            if get != give && self.bank[get] > 0 {
                                out.push(Action::MaritimeTrade { give, get });
                            }
                        }
                    }
                }
                if let Some(offer) = self.trade {
                    for q in 0..self.num_players() as u8 {
                        if offer.responses[q as usize] == TradeResponse::Accepted
                            && ps.hand.contains(&offer.give)
                            && self.players[q as usize].hand.contains(&offer.get)
                        {
                            out.push(Action::ConfirmTrade { partner: q });
                        }
                    }
                    out.push(Action::CancelTrade);
                }
            }
            Phase::RoadBuilding { .. } => {
                if ps.roads_left > 0 {
                    for e in edges_in(self.road_spots(player)) {
                        out.push(Action::BuildRoad(e));
                    }
                }
                if ps.ships_left > 0 {
                    for e in edges_in(self.ship_spots(player)) {
                        out.push(Action::BuildShip(e));
                    }
                }
            }
            Phase::SpecialBuild { .. } => self.push_builds(player, out),
            Phase::GameOver => {}
        }
    }

    /// `EndTurn` plus every affordable build and development card purchase.
    fn push_builds(&self, player: u8, out: &mut Vec<Action>) {
        let ps = &self.players[player as usize];
        out.push(Action::EndTurn);
        if ps.roads_left > 0 && ps.hand.contains(&cost::ROAD) {
            for e in edges_in(self.road_spots(player)) {
                out.push(Action::BuildRoad(e));
            }
        }
        if ps.ships_left > 0 && ps.hand.contains(&cost::SHIP) {
            for e in edges_in(self.ship_spots(player)) {
                out.push(Action::BuildShip(e));
            }
        }
        if ps.settlements_left > 0 && ps.hand.contains(&cost::SETTLEMENT) {
            for v in vertices_in(self.settlement_spots(player, true)) {
                out.push(Action::BuildSettlement(v));
            }
        }
        if ps.cities_left > 0 && ps.hand.contains(&cost::CITY) {
            for v in vertices_in(self.settlements[player as usize]) {
                out.push(Action::BuildCity(v));
            }
        }
        if self.dev_deck_len > 0 && ps.hand.contains(&cost::DEV_CARD) {
            out.push(Action::BuyDevCard);
        }
    }

    fn push_dev_card_plays(&self, player: u8, out: &mut Vec<Action>) {
        if self.dev_card_played {
            return;
        }
        let ps = &self.players[player as usize];
        let has = |c: DevCard| ps.dev_cards[c.index()] > 0;
        if has(DevCard::Knight) {
            out.push(Action::PlayKnight);
        }
        let route = (ps.roads_left > 0 && self.road_spots(player) != 0)
            || (ps.ships_left > 0 && self.ship_spots(player) != 0);
        if has(DevCard::RoadBuilding) && route {
            out.push(Action::PlayRoadBuilding);
        }
        if has(DevCard::YearOfPlenty) {
            match self.bank.total() {
                0 => {}
                1 => {
                    let r = self.bank.nth_card(0).unwrap();
                    out.push(Action::PlayYearOfPlenty(ResourceSet::single(r, 1)));
                }
                _ => {
                    for a in 0..5 {
                        for b in a..5 {
                            let mut set = ResourceSet::EMPTY;
                            set.0[a] += 1;
                            set.0[b] += 1;
                            if self.bank.contains(&set) {
                                out.push(Action::PlayYearOfPlenty(set));
                            }
                        }
                    }
                }
            }
        }
        if has(DevCard::Monopoly) {
            for r in Resource::ALL {
                out.push(Action::PlayMonopoly(r));
            }
        }
    }
}

impl GameState {
    /// Every way to pick the gold `player` is owed from the bank.
    fn push_gold_choices(&self, player: u8, out: &mut Vec<Action>) {
        let owed = self.pending_gold[player as usize] as u32;
        if owed == 0 {
            return;
        }
        let want = owed.min(self.bank.total()) as u8;
        push_subsets(self.bank, want, Action::ChooseGold, out);
    }
}

/// Every distinct way to discard `need` cards from `hand`.
fn push_discards(hand: ResourceSet, need: u8, out: &mut Vec<Action>) {
    push_subsets(hand, need, Action::Discard, out);
}

/// Every distinct set of `need` cards taken from `from`.
fn push_subsets(
    from: ResourceSet,
    need: u8,
    action: fn(ResourceSet) -> Action,
    out: &mut Vec<Action>,
) {
    fn rec(
        i: usize,
        remaining: u8,
        from: &[u8; 5],
        cur: &mut [u8; 5],
        action: fn(ResourceSet) -> Action,
        out: &mut Vec<Action>,
    ) {
        if i == 4 {
            if remaining <= from[4] {
                cur[4] = remaining;
                out.push(action(ResourceSet(*cur)));
            }
            return;
        }
        let rest: u8 = from[i + 1..].iter().sum();
        let lo = remaining.saturating_sub(rest);
        let hi = from[i].min(remaining);
        for c in lo..=hi {
            cur[i] = c;
            rec(i + 1, remaining - c, from, cur, action, out);
        }
    }
    rec(0, need, &from.0, &mut [0; 5], action, out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discard_enumeration_is_exact() {
        let mut out = Vec::new();
        push_discards(ResourceSet::new(2, 1, 0, 0, 1), 2, &mut out);
        // {BB, BL, BO, LO}
        assert_eq!(out.len(), 4);
        for a in &out {
            let Action::Discard(s) = a else { panic!() };
            assert_eq!(s.total(), 2);
        }
        push_discards(ResourceSet::new(3, 3, 3, 3, 3), 7, &mut out);
        let distinct: std::collections::HashSet<_> = out.iter().collect();
        assert_eq!(distinct.len(), out.len());
    }
}
