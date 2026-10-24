//! The computer opponent offered from the quick-match queue when nobody else is around.
//!
//! It plays the same way as the practice opponent in the browser (this is a port of it): it
//! scores every card in its hand as "add to a trail" or "discard", adds a little random
//! noise (more noise = weaker), picks the best, then decides where to draw from. It only
//! ever asks the rules engine for moves the rules allow, and it never sees the human's hand.

use crate::game::{can_extend, color, value, Card, Game, Phase, COLORS};
use rand::Rng;

/// Trophy counts at which the computer gets sharper (the same arenas as the lobby).
const TIER_AT: [u32; 7] = [0, 100, 250, 450, 700, 1000, 1400];
/// Random noise added to each choice's score, weakest to strongest tier.
const NOISE: [f32; 7] = [8.0, 6.0, 4.5, 3.2, 2.0, 1.0, 0.3];
pub const TIERS: usize = TIER_AT.len();

pub fn tier_for(trophies: u32) -> usize {
    TIER_AT.iter().rposition(|&at| trophies >= at).unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    Rush,
    Sabotage,
    Play(Card, bool),
    Draw(Option<usize>),
}

impl Step {
    /// How long the computer "thinks" after this step before its next one, in milliseconds.
    pub fn pause_after_ms(self) -> u64 {
        match self {
            Step::Rush => 1200,
            Step::Sabotage => 1300,
            Step::Play(..) => 800,
            Step::Draw(_) => 0,
        }
    }
}

/// Where the computer is within its own turn: optional Rush, optional Sabotage, play, draw.
#[derive(Debug, Default)]
pub struct Brain {
    stage: u8,
}

impl Brain {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forget any half-finished turn (the other player moved, or a new game began).
    pub fn reset(&mut self) {
        self.stage = 0;
    }

    /// The next thing to do on the computer's turn.
    pub fn next<R: Rng + ?Sized>(&mut self, g: &Game, seat: usize, tier: usize, rng: &mut R) -> Step {
        if g.phase == Phase::Draw {
            self.stage = 0;
            return Step::Draw(choose_draw(g, seat));
        }
        if self.stage >= 3 {
            self.stage = 0;
        }
        loop {
            match self.stage {
                0 => {
                    self.stage = 1;
                    if g.rush_left[seat] > 0 && g.rush_next.is_none() && g.deck.len() <= 34 && rng.gen::<f32>() < 0.2 {
                        return Step::Rush;
                    }
                }
                1 => {
                    self.stage = 2;
                    let left = g.deck.len();
                    if g.sabotage_left[seat] > 0 && (6..=30).contains(&left) && !g.hands[1 - seat].is_empty() && rng.gen::<f32>() < 0.25 {
                        return Step::Sabotage;
                    }
                }
                _ => {
                    self.stage = 3;
                    let (card, to_column) = choose_play(g, seat, NOISE[tier.min(TIERS - 1)], rng);
                    return Step::Play(card, to_column);
                }
            }
        }
    }
}

/// Highest number in a trail, at least 1 (wagers count as 0).
fn last_num(col: &[Card]) -> i32 {
    col.iter().map(|&c| i32::from(value(c))).max().unwrap_or(0).max(1)
}

/// The best card to play and whether to add it to a trail (true) or discard it (false).
pub fn choose_play<R: Rng + ?Sized>(g: &Game, seat: usize, noise: f32, rng: &mut R) -> (Card, bool) {
    let opp = 1 - seat;
    let hand = &g.hands[seat];
    let left = g.deck.len() as f32;
    let future = left.min(30.0) / 30.0 * 9.0;
    let mut best: Option<(f32, Card, bool)> = None;
    let mut take = |mut score: f32, card: Card, to_column: bool, rng: &mut R| {
        score += (rng.gen::<f32>() * 2.0 - 1.0) * noise;
        if best.is_none_or(|(b, _, _)| score > b) {
            best = Some((score, card, to_column));
        }
    };
    for &id in hand {
        let c = color(id);
        let v = i32::from(value(id));
        let mine = &g.columns[seat][c];
        let same: Vec<Card> = hand.iter().copied().filter(|&k| color(k) == c && k != id).collect();
        let numbered_same = || same.iter().filter(|&&k| value(k) != 0);
        if can_extend(mine, id) {
            if v == 0 {
                let pot: i32 = numbered_same().map(|&k| i32::from(value(k))).sum();
                take(if pot as f32 + future - 20.0 >= 6.0 && left > 18.0 { 4.0 } else { -12.0 }, id, true, rng);
            } else if !mine.is_empty() {
                let ln = last_num(mine);
                let skipped = same.iter().filter(|&&k| i32::from(value(k)) > ln && i32::from(value(k)) < v).count() as f32;
                take(9.0 - (v - ln - 1) as f32 * 0.9 - skipped * 5.0, id, true, rng);
            } else {
                let pot = v + same.iter().filter(|&&k| i32::from(value(k)) > v).map(|&k| i32::from(value(k))).sum::<i32>();
                let wager_waiting = same.iter().any(|&k| value(k) == 0);
                let proj = pot as f32 + future - 20.0;
                let base = if proj > 0.0 { proj * 0.45 } else { proj * 0.8 };
                take(base - (v - 2) as f32 * 0.4 - if wager_waiting { 3.0 } else { 0.0 } - if left < 10.0 { 4.0 } else { 0.0 }, id, true, rng);
            }
        }
        let theirs = &g.columns[opp][c];
        let mut d = 1.0f32;
        if v != 0 && can_extend(theirs, id) {
            d -= if theirs.is_empty() { 1.5 } else { 6.0 + if v >= 8 { 2.0 } else { 0.0 } };
        }
        if !mine.is_empty() && v > last_num(mine) {
            d -= 5.0;
        }
        if v == 0 && mine.is_empty() && numbered_same().count() >= 3 {
            d -= 3.0;
        }
        if mine.is_empty() && v != 0 && same.is_empty() {
            d += 1.5;
        }
        take(d - 0.01 * v as f32, id, false, rng);
    }
    best.map_or((hand.first().copied().unwrap_or(0), false), |(_, card, to_column)| (card, to_column))
}

/// Where to draw from: a discard pile that extends one of its trails, else the deck.
pub fn choose_draw(g: &Game, seat: usize) -> Option<usize> {
    let mut pick = None;
    let mut best = 0.0f32;
    for c in 0..COLORS {
        if g.just_discarded == Some(c) {
            continue;
        }
        let Some(&top) = g.discards[c].last() else { continue };
        let mine = &g.columns[seat][c];
        if !mine.is_empty() && can_extend(mine, top) {
            let score = 6.0 - (i32::from(value(top)) - last_num(mine)) as f32;
            if score > best {
                best = score;
                pick = Some(c);
            }
        }
    }
    // Near the end, when ahead, leave the deck alone so the game does not finish sooner.
    if pick.is_none() && g.deck.len() <= 3 && g.total(seat) > g.total(1 - seat) {
        pick = (0..COLORS).find(|&c| g.just_discarded != Some(c) && !g.discards[c].is_empty());
    }
    pick
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};
    use std::time::Instant;

    #[test]
    fn tiers_follow_the_lobby_arenas() {
        assert_eq!(tier_for(0), 0);
        assert_eq!(tier_for(99), 0);
        assert_eq!(tier_for(100), 1);
        assert_eq!(tier_for(1399), 5);
        assert_eq!(tier_for(1400), 6);
        assert_eq!(tier_for(u32::MAX), 6);
    }

    /// Two computers play whole games against each other through the real rules engine.
    /// Any move the rules refuse would fail the unwrap, so this checks the bot is always legal.
    #[test]
    fn computers_finish_whole_games_with_only_legal_moves() {
        let mut played = 0;
        for game_no in 0..300u64 {
            let mut rng = StdRng::seed_from_u64(game_no);
            let now = Instant::now();
            let mut g = Game::new(&mut rng, (game_no % 2) as usize, None, now);
            let mut brains = [Brain::new(), Brain::new()];
            let tiers = [(game_no % 7) as usize, ((game_no / 7) % 7) as usize];
            let mut guard = 0;
            while !g.over {
                guard += 1;
                assert!(guard < 5000, "game {game_no} did not finish");
                let seat = g.turn;
                match brains[seat].next(&g, seat, tiers[seat], &mut rng) {
                    Step::Rush => g.rush(seat).unwrap(),
                    Step::Sabotage => g.sabotage(seat, &mut rng).unwrap(),
                    Step::Play(card, to_column) => g.play(seat, card, to_column).unwrap(),
                    Step::Draw(pile) => g.draw(seat, pile, now).unwrap(),
                }
                brains[1 - seat].reset();
            }
            played += 1;
        }
        assert_eq!(played, 300);
    }

    #[test]
    fn a_strong_computer_beats_a_weak_one_most_of_the_time() {
        let (mut strong_wins, mut weak_wins) = (0, 0);
        for game_no in 0..400u64 {
            let mut rng = StdRng::seed_from_u64(10_000 + game_no);
            let now = Instant::now();
            let mut g = Game::new(&mut rng, (game_no % 2) as usize, None, now);
            let mut brains = [Brain::new(), Brain::new()];
            let strong = (game_no % 2) as usize; // strong plays seat 0 in even games, seat 1 in odd
            let tiers = if strong == 0 { [6, 0] } else { [0, 6] };
            while !g.over {
                let seat = g.turn;
                match brains[seat].next(&g, seat, tiers[seat], &mut rng) {
                    Step::Rush => g.rush(seat).unwrap(),
                    Step::Sabotage => g.sabotage(seat, &mut rng).unwrap(),
                    Step::Play(card, to_column) => g.play(seat, card, to_column).unwrap(),
                    Step::Draw(pile) => g.draw(seat, pile, now).unwrap(),
                }
                brains[1 - seat].reset();
            }
            match g.total(strong).cmp(&g.total(1 - strong)) {
                std::cmp::Ordering::Greater => strong_wins += 1,
                std::cmp::Ordering::Less => weak_wins += 1,
                std::cmp::Ordering::Equal => {}
            }
        }
        assert!(strong_wins > weak_wins * 3 / 2, "strong {strong_wins} vs weak {weak_wins}");
    }

    #[test]
    fn draw_phase_always_answers_with_a_draw_and_powers_are_used_at_most_once() {
        let mut rng = StdRng::seed_from_u64(7);
        let now = Instant::now();
        let mut g = Game::new(&mut rng, 0, None, now);
        let mut brain = Brain::new();
        let (mut rushes, mut sabs) = (0, 0);
        while !g.over {
            let seat = g.turn;
            let phase_before = g.phase;
            let step = brain.next(&g, seat, 3, &mut rng);
            if phase_before == Phase::Draw {
                assert!(matches!(step, Step::Draw(_)));
            }
            match step {
                Step::Rush => {
                    rushes += 1;
                    g.rush(seat).unwrap()
                }
                Step::Sabotage => {
                    sabs += 1;
                    g.sabotage(seat, &mut rng).unwrap()
                }
                Step::Play(card, to_column) => g.play(seat, card, to_column).unwrap(),
                Step::Draw(pile) => g.draw(seat, pile, now).unwrap(),
            }
        }
        // Both seats were driven by one brain here, so at most one power of each kind per seat.
        assert!(rushes <= 2 && sabs <= 2);
    }

    #[test]
    fn steps_pause_like_a_person_would() {
        assert!(Step::Play(0, true).pause_after_ms() > 0);
        assert_eq!(Step::Draw(None).pause_after_ms(), 0);
    }
}
