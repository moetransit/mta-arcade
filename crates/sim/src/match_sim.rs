//! The 1v1 match: movement + instagib railgun + bemani scoring, fully
//! deterministic. This struct IS the rollback state — clone-snapshotted by
//! GGRS, advanced identically on every peer from inputs alone.

use crate::arena::{Aabb, SPAWNS};
use crate::judgment::{judge, BeatGrid, Judgment};
use crate::movement::{step, NetInput, PlayerState, BTN_FIRE, EYE_HEIGHT};

/// Eighth-note cadence: ~12.5 ticks at 144bpm; 12 gives a tick of slack.
/// (playtest-driven, twice: 1.2s instagib -> beat -> eighth)
pub const FIRE_COOLDOWN_TICKS: u32 = 12;
pub const RAY_RANGE: f32 = 200.0;
/// First to this many points wins (design doc mode 2).
pub const POINT_LIMIT: u32 = 30;
/// Results screen duration before auto-rematch (10s at 60Hz). Both players
/// pressing fire during results rematches sooner.
pub const RESULTS_TICKS: u32 = 600;

/// One fire event, kept in state so peers (and rollback) agree on the feed.
#[derive(Clone, Debug, PartialEq)]
pub struct FireRecord {
    pub tick: u32,
    pub shooter: usize,
    pub hit: bool,
    pub judgment: Judgment,
    pub origin: [f32; 3],
    pub dir: [f32; 3],
}

/// Per-player combo chain, sim-owned so netplay agrees on it.
/// Display-only for now (no score multiplier) but deterministic by
/// construction, so a multiplier is a one-line change later.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ComboState {
    pub count: u32,
    pub consec: u32,
    pub last_slot: Option<i64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatchState {
    pub players: [PlayerState; 2],
    pub cooldowns: [u32; 2],
    pub frags: [u32; 2],
    pub points: [u32; 2],
    pub tick: u32,
    pub last_fire: Option<FireRecord>,
    pub winner: Option<usize>,
    /// Counts down during the results screen once a winner exists.
    pub results_ticks: u32,
    pub rematch_ready: [bool; 2],
    /// Increments on each respawn — presentation watches it to reset the view.
    pub spawn_gen: [u32; 2],
    pub combo: [ComboState; 2],
    /// Increments every time a fresh match starts (rematch counter).
    pub round: u32,
}

impl MatchState {
    pub fn new() -> Self {
        Self {
            players: [
                PlayerState::spawn(SPAWNS[0].0, SPAWNS[0].1),
                PlayerState::spawn(SPAWNS[1].0, SPAWNS[1].1),
            ],
            cooldowns: [0; 2],
            frags: [0; 2],
            points: [0; 2],
            tick: 0,
            last_fire: None,
            winner: None,
            results_ticks: 0,
            rematch_ready: [false; 2],
            spawn_gen: [0; 2],
            combo: [ComboState::default(), ComboState::default()],
            round: 0,
        }
    }

    /// Fresh match, preserving the tick stream and bumping the round counter.
    fn rematch(&mut self) {
        let tick = self.tick;
        let round = self.round + 1;
        *self = MatchState::new();
        self.tick = tick;
        self.round = round;
    }

    /// Track time for judgment: the sim tick clock, looped over the track.
    pub fn track_time(&self, track_duration: f64) -> f64 {
        (self.tick as f64 / crate::TICK_HZ as f64) % track_duration
    }
}

impl Default for MatchState {
    fn default() -> Self {
        Self::new()
    }
}

fn eye_forward(p: &PlayerState) -> ([f32; 3], [f32; 3]) {
    let origin = [p.pos[0], p.pos[1] + EYE_HEIGHT, p.pos[2]];
    let (sy, cy) = (p.yaw.sin(), p.yaw.cos());
    let (sp, cp) = (p.pitch.sin(), p.pitch.cos());
    ([origin[0], origin[1], origin[2]], [-cp * sy, sp, -cp * cy])
}

/// Ray vs AABB slab test → entry distance if hit within `max_t`.
fn ray_aabb(origin: [f32; 3], dir: [f32; 3], b: &Aabb, max_t: f32) -> Option<f32> {
    let mut t0 = 0.0_f32;
    let mut t1 = max_t;
    for i in 0..3 {
        if dir[i].abs() < 1e-8 {
            if origin[i] < b.0[i] || origin[i] > b.1[i] {
                return None;
            }
            continue;
        }
        let inv = 1.0 / dir[i];
        let (mut near, mut far) = ((b.0[i] - origin[i]) * inv, (b.1[i] - origin[i]) * inv);
        if near > far {
            core::mem::swap(&mut near, &mut far);
        }
        t0 = t0.max(near);
        t1 = t1.min(far);
        if t0 > t1 {
            return None;
        }
    }
    Some(t0)
}

fn player_hitbox(p: &PlayerState) -> Aabb {
    use crate::movement::{PLAYER_HALF_XZ, PLAYER_HEIGHT};
    (
        [
            p.pos[0] - PLAYER_HALF_XZ,
            p.pos[1],
            p.pos[2] - PLAYER_HALF_XZ,
        ],
        [
            p.pos[0] + PLAYER_HALF_XZ,
            p.pos[1] + PLAYER_HEIGHT,
            p.pos[2] + PLAYER_HALF_XZ,
        ],
    )
}

/// Respawn at whichever spawn is farther from the killer.
fn respawn_far_from(killer_pos: [f32; 3]) -> PlayerState {
    let d = |s: &([f32; 3], f32)| {
        (0..3)
            .map(|i| (s.0[i] - killer_pos[i]) * (s.0[i] - killer_pos[i]))
            .sum::<f32>()
    };
    let s = if d(&SPAWNS[0]) >= d(&SPAWNS[1]) {
        SPAWNS[0]
    } else {
        SPAWNS[1]
    };
    PlayerState::spawn(s.0, s.1)
}

/// Advance one tick of the match. Deterministic; the only entry point.
pub fn step_match(
    state: &mut MatchState,
    inputs: [NetInput; 2],
    arena: &[Aabb],
    grid: &BeatGrid,
    track_duration: f64,
) {
    // results phase: countdown, fire-to-rematch, then a fresh round
    if state.winner.is_some() {
        for (i, input) in inputs.iter().enumerate() {
            if input.buttons & BTN_FIRE != 0 {
                state.rematch_ready[i] = true;
            }
        }
        state.results_ticks = state.results_ticks.saturating_sub(1);
        if state.results_ticks == 0 || (state.rematch_ready[0] && state.rematch_ready[1]) {
            state.rematch();
        }
        state.tick += 1;
        return;
    }

    for (i, input) in inputs.iter().enumerate() {
        step(&mut state.players[i], input, arena);
        state.cooldowns[i] = state.cooldowns[i].saturating_sub(1);
    }

    // fires resolve in player order — deterministic tie-break; a double-kill
    // on the same tick favors player 0 (documented quirk until sudden-death)
    for (i, input) in inputs.iter().enumerate() {
        if input.buttons & BTN_FIRE == 0 || state.cooldowns[i] > 0 {
            continue;
        }
        state.cooldowns[i] = FIRE_COOLDOWN_TICKS;
        let (origin, dir) = eye_forward(&state.players[i]);

        // nearest wall the beam stops at
        let wall_t = arena
            .iter()
            .filter_map(|b| ray_aabb(origin, dir, b, RAY_RANGE))
            .fold(RAY_RANGE, f32::min);
        let victim = 1 - i;
        let hit = ray_aabb(
            origin,
            dir,
            &player_hitbox(&state.players[victim]),
            RAY_RANGE,
        )
        .map(|t| t < wall_t)
        .unwrap_or(false);

        let t = state.track_time(track_duration);
        let judgment = judge(grid, t);

        // combo: off-rhythm firing breaks it; on-rhythm kills grow it
        if judgment == Judgment::OffRhythm {
            state.combo[i] = ComboState::default();
        }

        if hit {
            state.frags[i] += 1;
            state.points[i] += judgment.points();
            if judgment != Judgment::OffRhythm {
                let slot = grid.eighth_slot(t);
                let combo = &mut state.combo[i];
                combo.count += 1;
                combo.consec = match (combo.last_slot, slot) {
                    (Some(last), Some(now)) if now == last + 1 => combo.consec + 1,
                    _ => 1,
                };
                combo.last_slot = slot;
            }
            state.players[victim] = respawn_far_from(state.players[i].pos);
            state.spawn_gen[victim] = state.spawn_gen[victim].wrapping_add(1);
            state.combo[victim] = ComboState::default();
            if state.points[i] >= POINT_LIMIT {
                state.winner = Some(i);
                state.results_ticks = RESULTS_TICKS;
            }
        }
        state.last_fire = Some(FireRecord {
            tick: state.tick,
            shooter: i,
            hit,
            judgment,
            origin,
            dir,
        });
    }

    state.tick += 1;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::graybox;
    use crate::movement::BTN_FIRE;

    fn grid() -> BeatGrid {
        let period = 60.0 / 144.0;
        BeatGrid::new((0..1000).map(|i| 0.006 + i as f64 * period).collect())
    }

    #[test]
    fn spawn_fire_frags_across_the_arena() {
        let arena = graybox();
        let g = grid();
        let mut m = MatchState::new();
        // settle both players onto the floor
        for _ in 0..30 {
            step_match(&mut m, [NetInput::default(); 2], &arena, &g, 420.0);
        }
        // p0 fires straight ahead: spawns face each other, same eye height
        let fire = NetInput {
            buttons: BTN_FIRE,
            ..Default::default()
        };
        step_match(&mut m, [fire, NetInput::default()], &arena, &g, 420.0);
        assert_eq!(m.frags[0], 1, "p0 should frag p1 across the arena");
        assert!(m.points[0] >= 1);
        let rec = m.last_fire.as_ref().expect("fire record");
        assert!(rec.hit);
        // victim respawned away from the killer
        assert!(m.players[1].pos[2] < 0.0);
    }

    #[test]
    fn cooldown_gates_fire_rate() {
        let arena = graybox();
        let g = grid();
        let mut m = MatchState::new();
        for _ in 0..30 {
            step_match(&mut m, [NetInput::default(); 2], &arena, &g, 420.0);
        }
        let fire = NetInput {
            buttons: BTN_FIRE,
            ..Default::default()
        };
        // hold fire for 60 ticks: shots every 12 ticks -> 5 frags
        for _ in 0..60 {
            step_match(&mut m, [fire, NetInput::default()], &arena, &g, 420.0);
        }
        assert_eq!(m.frags[0], 5);
    }

    #[test]
    fn full_round_flow() {
        let arena = graybox();
        let g = grid();
        let mut m = MatchState::new();
        for _ in 0..30 {
            step_match(&mut m, [NetInput::default(); 2], &arena, &g, 420.0);
        }
        let fire = NetInput {
            buttons: BTN_FIRE,
            ..Default::default()
        };
        // p0 farms the stationary p1 until the point limit trips
        let mut safety = 0;
        while m.winner.is_none() {
            step_match(&mut m, [fire, NetInput::default()], &arena, &g, 420.0);
            safety += 1;
            assert!(safety < 3600, "no winner within a minute of farming");
        }
        assert_eq!(m.winner, Some(0));
        assert!(m.results_ticks > 0);
        assert!(m.spawn_gen[1] > 0, "victim respawns were counted");
        // both press fire during results -> rematch on that very tick
        let mut safety = 0;
        while m.round == 0 {
            step_match(&mut m, [fire, fire], &arena, &g, 420.0);
            safety += 1;
            assert!(safety < 5, "rematch should trigger promptly");
        }
        assert_eq!(m.winner, None, "rematch resets the winner");
        assert_eq!(m.round, 1);
        assert_eq!(m.points, [0, 0], "fresh round starts clean");
        assert_eq!(m.frags, [0, 0]);
    }

    #[test]
    fn combo_counts_in_sim() {
        let arena = graybox();
        let g = grid();
        let mut m = MatchState::new();
        for _ in 0..30 {
            step_match(&mut m, [NetInput::default(); 2], &arena, &g, 420.0);
        }
        let fire = NetInput {
            buttons: BTN_FIRE,
            ..Default::default()
        };
        for _ in 0..60 {
            step_match(&mut m, [fire, NetInput::default()], &arena, &g, 420.0);
        }
        // 5 kills; combo counts only the on-rhythm ones and never exceeds kills
        assert!(m.combo[0].count <= m.frags[0]);
        assert_eq!(m.frags[0], 5);
    }

    /// Golden 2p-match hash: the full match layer joins the rollback contract.
    #[test]
    fn determinism_hash() {
        fn fnv1a(h: u64, bytes: &[u8]) -> u64 {
            bytes.iter().fold(h, |h, &b| {
                (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
            })
        }
        let run = || {
            let arena = graybox();
            let g = grid();
            let mut m = MatchState::new();
            let mut h = 0xcbf2_9ce4_8422_2325_u64;
            for i in 0..3600_u64 {
                let mk = |seed: u64| NetInput {
                    buttons: ((seed % 63) as u8) & 0b11_1111,
                    yaw_mrad: ((seed * 41) % 6283) as i16 - 3141,
                    pitch_mrad: ((seed * 17) % 2000) as i16 - 1000,
                };
                step_match(
                    &mut m,
                    [mk(i), mk(i.wrapping_mul(7) + 3)],
                    &arena,
                    &g,
                    420.0,
                );
                for p in &m.players {
                    for v in p.pos.iter() {
                        h = fnv1a(h, &v.to_le_bytes());
                    }
                }
                h = fnv1a(h, &[m.frags[0] as u8, m.frags[1] as u8]);
                // weapon-layer state too: positions alone are blind to
                // cooldown/scoring changes when scripted shots whiff
                h = fnv1a(h, &m.cooldowns[0].to_le_bytes());
                h = fnv1a(h, &m.cooldowns[1].to_le_bytes());
                h = fnv1a(h, &m.points[0].to_le_bytes());
            }
            h
        };
        assert_eq!(run(), run());
        assert_eq!(run(), GOLDEN);
    }

    const GOLDEN: u64 = 15094769557619775746;
}
