//! Bemani-style kill judgment against a track's beat grid.
//!
//! Windows are expressed in seconds here but chosen as tick multiples
//! (±1 tick MARVELOUS, ±3 ticks GREAT at 60Hz). Judgment must be identical
//! on every peer: same fire time + same grid = same verdict, always.

/// A track's beat grid: strictly increasing beat times in seconds.
#[derive(Debug, Clone, PartialEq)]
pub struct BeatGrid {
    pub beat_times: Vec<f64>,
}

impl BeatGrid {
    pub fn new(beat_times: Vec<f64>) -> Self {
        debug_assert!(beat_times.windows(2).all(|w| w[0] < w[1]));
        Self { beat_times }
    }

    /// Absolute distance (seconds) from `t` to the nearest beat.
    pub fn distance_to_beat(&self, t: f64) -> Option<f64> {
        nearest_abs(&self.beat_times, t)
    }

    /// Absolute distance (seconds) from `t` to the nearest offbeat
    /// (midpoint between adjacent beats).
    pub fn distance_to_offbeat(&self, t: f64) -> Option<f64> {
        let i = self.beat_times.partition_point(|&b| b <= t);
        let mut best: Option<f64> = None;
        for off in 0..=2usize {
            let Some(j) = (i + off).checked_sub(2) else {
                continue;
            };
            if let (Some(&a), Some(&b)) = (self.beat_times.get(j), self.beat_times.get(j + 1)) {
                let mid = (a + b) / 2.0;
                let d = (t - mid).abs();
                best = Some(best.map_or(d, |x: f64| x.min(d)));
            }
        }
        best
    }
}

fn nearest_abs(sorted: &[f64], t: f64) -> Option<f64> {
    let i = sorted.partition_point(|&b| b <= t);
    let prev = i.checked_sub(1).and_then(|j| sorted.get(j));
    let next = sorted.get(i);
    match (prev, next) {
        (Some(&p), Some(&n)) => Some((t - p).min(n - t)),
        (Some(&p), None) => Some(t - p),
        (None, Some(&n)) => Some(n - t),
        (None, None) => None,
    }
}

/// MARVELOUS window: ±1 tick at 60Hz.
pub const MARVELOUS_WINDOW_S: f64 = 0.017;
/// GREAT window: ±3 ticks at 60Hz.
pub const GREAT_WINDOW_S: f64 = 0.050;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Judgment {
    /// Frame-perfect on the beat.
    Marvelous,
    /// Frame-perfect on the offbeat.
    MarvOff,
    /// Close to either.
    Great,
    /// A kill is still a kill.
    OffRhythm,
}

impl Judgment {
    pub fn points(self) -> u32 {
        match self {
            Judgment::Marvelous => 5,
            Judgment::MarvOff => 4,
            Judgment::Great => 2,
            Judgment::OffRhythm => 1,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Judgment::Marvelous => "MARVELOUS",
            Judgment::MarvOff => "MARV·OFF",
            Judgment::Great => "GREAT",
            Judgment::OffRhythm => "...",
        }
    }
}

/// Judge a kill at time `t` (seconds on the track clock) against the grid.
pub fn judge(grid: &BeatGrid, t: f64) -> Judgment {
    let on = grid.distance_to_beat(t);
    let off = grid.distance_to_offbeat(t);
    match (on, off) {
        (Some(b), _) if b <= MARVELOUS_WINDOW_S => Judgment::Marvelous,
        (_, Some(o)) if o <= MARVELOUS_WINDOW_S => Judgment::MarvOff,
        (Some(b), Some(o)) if b.min(o) <= GREAT_WINDOW_S => Judgment::Great,
        (Some(b), None) if b <= GREAT_WINDOW_S => Judgment::Great,
        _ => Judgment::OffRhythm,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid_144() -> BeatGrid {
        // 144 bpm from t=0.006 (disco machine gun's real shape)
        let period = 60.0 / 144.0;
        BeatGrid::new((0..1000).map(|i| 0.006 + i as f64 * period).collect())
    }

    #[test]
    fn windows() {
        let g = grid_144();
        let beat = g.beat_times[10];
        let period = 60.0 / 144.0;
        assert_eq!(judge(&g, beat), Judgment::Marvelous);
        assert_eq!(judge(&g, beat + 0.016), Judgment::Marvelous);
        assert_eq!(judge(&g, beat - 0.016), Judgment::Marvelous);
        assert_eq!(judge(&g, beat + 0.030), Judgment::Great);
        assert_eq!(judge(&g, beat + 0.090), Judgment::OffRhythm);
        assert_eq!(judge(&g, beat + period / 2.0), Judgment::MarvOff);
        assert_eq!(judge(&g, beat + period / 2.0 + 0.016), Judgment::MarvOff);
        assert_eq!(judge(&g, beat + period / 2.0 + 0.040), Judgment::Great);
    }

    #[test]
    fn edges() {
        let g = BeatGrid::new(vec![1.0, 2.0]);
        assert_eq!(judge(&g, 0.0), Judgment::OffRhythm); // 1s before first beat
        assert_eq!(judge(&g, 0.999), Judgment::Marvelous);
        assert_eq!(judge(&g, 2.01), Judgment::Marvelous);
        assert_eq!(judge(&BeatGrid::new(vec![]), 1.0), Judgment::OffRhythm);
    }

    /// The rollback contract: identical inputs → bit-identical verdict
    /// sequence, run twice, pinned to a golden value. FNV-1a by hand:
    /// std's DefaultHasher is not stable across rust versions.
    #[test]
    fn determinism_hash() {
        fn fnv1a(h: u64, bytes: &[u8]) -> u64 {
            bytes.iter().fold(h, |h, &b| {
                (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
            })
        }

        let run = || {
            let g = grid_144();
            let mut h = 0xcbf2_9ce4_8422_2325_u64;
            // scripted "match": fires at pseudo-random-but-fixed times
            let mut t = 0.0_f64;
            for i in 0..10_000_u64 {
                t += 0.037 + (i % 7) as f64 * 0.013;
                h = fnv1a(h, &[judge(&g, t).points() as u8]);
                h = fnv1a(h, &crate::secs_to_ticks(t).to_le_bytes());
            }
            h
        };
        assert_eq!(run(), run());
        // golden value: changing judgment behavior must be a conscious act
        assert_eq!(run(), GOLDEN);
    }

    const GOLDEN: u64 = 5540479864824869777;
}
