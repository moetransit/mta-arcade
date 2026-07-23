//! The vibe layer: audio analysis feeding the world's heartbeat.
//!
//! Browser: an `<audio>` element streams the track through a Web Audio
//! `AnalyserNode`; per frame we fold the FFT into four band energies.
//! Native dev builds get silent zeroed bands (the dev loop is the browser).
//!
//! Everything here is cosmetic-only by design — gameplay must never read
//! these values (see design doc §5: FFT visuals are local, beats are sim).

use bevy::prelude::*;

/// Per-frame band energies in 0..1, smoothed. Cosmetic consumers only.
#[derive(Resource, Default)]
pub struct AudioBands {
    pub bass: f32,
    pub lowmid: f32,
    pub highmid: f32,
    pub treble: f32,
}

/// The track we ship with arena #1. Audio streams as an asset; the analyzed
/// beat grid is embedded in the binary (10KB) so cosmetics never wait on a fetch.
const TRACK_SRC: &str = "assets/tracks/disco_machine_gun.ogg";
const BEATS_JSON: &str = include_str!("../assets/tracks/disco_machine_gun.beats.json");

/// What's currently playing, for the UI.
#[derive(Resource)]
pub struct NowPlaying {
    pub title: String,
    pub artist: String,
}

/// Track clock for beat pulses. `time_s` is the sample-accurate playhead
/// (AudioContext clock minus reported output latency), judged against the
/// track's fitted beat grid. `cal_offset_s` is the user's tap calibration.
#[derive(Resource, Default)]
pub struct BeatClock {
    pub beat_times: Vec<f64>,
    pub duration_s: f64,
    pub time_s: f64,
    pub playing: bool,
    pub cal_offset_s: f64,
    pub taps: Vec<f64>,
}

impl BeatClock {
    /// Playhead corrected by the user's tap calibration.
    pub fn effective_time(&self) -> f64 {
        self.time_s - self.cal_offset_s
    }

    /// 0 at each beat, rising to 1 just before the next: a saw for pulses.
    pub fn beat_phase(&self) -> f32 {
        let t = self.effective_time();
        let i = self.beat_times.partition_point(|&b| b <= t);
        if i == 0 || i >= self.beat_times.len() {
            return 1.0;
        }
        let (prev, next) = (self.beat_times[i - 1], self.beat_times[i]);
        (((t - prev) / (next - prev)).clamp(0.0, 1.0)) as f32
    }

    /// Signed offset (seconds) from `t` to the nearest beat.
    pub fn offset_to_nearest_beat(&self, t: f64) -> Option<f64> {
        let i = self.beat_times.partition_point(|&b| b <= t);
        let prev = i.checked_sub(1).map(|j| self.beat_times[j]);
        let next = self.beat_times.get(i).copied();
        match (prev, next) {
            (Some(p), Some(n)) => Some(if t - p < n - t { t - p } else { t - n }),
            (Some(p), None) => Some(t - p),
            (None, Some(n)) => Some(t - n),
            (None, None) => None,
        }
    }

    /// Record a tap at the current raw playhead; returns the running median
    /// calibration once at least 4 taps are in.
    pub fn record_tap(&mut self) -> Option<f64> {
        let off = self.offset_to_nearest_beat(self.time_s)?;
        // ignore wild taps (> 200ms off: probably not aimed at a beat)
        if off.abs() > 0.2 {
            return None;
        }
        self.taps.push(off);
        if self.taps.len() < 4 {
            return None;
        }
        let mut sorted = self.taps.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = sorted[sorted.len() / 2];
        self.cal_offset_s = median;
        Some(median)
    }
}

/// Persist / restore tap calibration (per device).
pub fn save_calibration(offset_s: f64) {
    #[cfg(target_arch = "wasm32")]
    web::save_calibration(offset_s);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = offset_s;
}

pub fn load_calibration() -> f64 {
    #[cfg(target_arch = "wasm32")]
    return web::load_calibration();
    #[cfg(not(target_arch = "wasm32"))]
    0.0
}

pub struct VibePlugin;

impl Plugin for VibePlugin {
    fn build(&self, app: &mut App) {
        let meta: serde_json::Value = serde_json::from_str(BEATS_JSON).expect("valid beats json");
        let beat_times = meta["beat_times"]
            .as_array()
            .expect("beat_times array")
            .iter()
            .filter_map(|v| v.as_f64())
            .collect();
        app.init_resource::<AudioBands>()
            .insert_resource(BeatClock {
                beat_times,
                duration_s: meta["duration_s"].as_f64().unwrap_or(0.0),
                cal_offset_s: load_calibration(),
                ..default()
            })
            .insert_resource(NowPlaying {
                title: meta["title"].as_str().unwrap_or("?").to_string(),
                artist: meta["artist"].as_str().unwrap_or("?").to_string(),
            })
            .add_systems(Update, sample_audio);
    }
}

/// Start (or resume) the track. Must be called from a user-gesture frame —
/// we piggyback on the cursor-grab click.
pub fn ensure_audio_started() {
    #[cfg(target_arch = "wasm32")]
    web::ensure_started();
}

/// Begin fetching + decoding the track NOW (no user gesture needed for
/// that) so the first click can start playback instantly.
pub fn preload_audio() {
    #[cfg(target_arch = "wasm32")]
    web::preload();
}

/// True once the track is decoded and ready for instant playback.
pub fn audio_ready() -> bool {
    #[cfg(target_arch = "wasm32")]
    return web::ready();
    #[cfg(not(target_arch = "wasm32"))]
    true
}

/// Restart the track from t=0 — the match's shared musical clock: both
/// peers call this on session start, aligning audio with sim tick 0.
pub fn restart_track() {
    #[cfg(target_arch = "wasm32")]
    web::restart();
}

fn sample_audio(mut bands: ResMut<AudioBands>, mut clock: ResMut<BeatClock>) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some((raw, time)) = web::sample() {
            // fft 1024 @ 44.1kHz -> 512 bins, ~43Hz each
            let mean = |range: core::ops::Range<usize>| {
                let slice = &raw[range];
                slice.iter().map(|&v| v as f32).sum::<f32>() / (slice.len() as f32 * 255.0)
            };
            let target = [mean(1..6), mean(6..24), mean(24..93), mean(93..512)];
            // fast attack, slow release: punchy but not strobing
            let follow = |cur: &mut f32, target: f32| {
                let rate = if target > *cur { 0.55 } else { 0.12 };
                *cur += (target - *cur) * rate;
            };
            follow(&mut bands.bass, target[0]);
            follow(&mut bands.lowmid, target[1]);
            follow(&mut bands.highmid, target[2]);
            follow(&mut bands.treble, target[3]);
            clock.time_s = time;
            clock.playing = true;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        // native dev: silence
        let _ = (&mut bands, &mut clock);
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{AnalyserNode, AudioBuffer, AudioBufferSourceNode, AudioContext};

    /// Playback via AudioBufferSourceNode on the AudioContext clock:
    /// sample-accurate playhead (see "A Tale of Two Clocks").
    /// Fetch+decode happen eagerly at startup (allowed pre-gesture); only
    /// playback waits for the first click — which is then instant.
    struct Loaded {
        ctx: AudioContext,
        analyser: AnalyserNode,
        buffer: AudioBuffer,
        source: Option<AudioBufferSourceNode>,
        start_time: f64,
        duration: f64,
        buf: Vec<u8>,
    }

    enum Phase {
        Idle,
        Loading { play_when_ready: bool },
        Ready(Loaded),
    }

    thread_local! {
        static AUDIO: RefCell<Phase> = const { RefCell::new(Phase::Idle) };
    }

    pub fn preload() {
        let should_load = AUDIO.with(|a| {
            let mut a = a.borrow_mut();
            match &*a {
                Phase::Idle => {
                    *a = Phase::Loading {
                        play_when_ready: false,
                    };
                    true
                }
                _ => false,
            }
        });
        if should_load {
            wasm_bindgen_futures::spawn_local(async {
                match load().await {
                    Ok(loaded) => AUDIO.with(|a| {
                        let mut a = a.borrow_mut();
                        let pending = matches!(
                            &*a,
                            Phase::Loading {
                                play_when_ready: true
                            }
                        );
                        *a = Phase::Ready(loaded);
                        if pending {
                            if let Phase::Ready(l) = &mut *a {
                                start_source(l);
                            }
                        }
                    }),
                    Err(err) => {
                        bevy::log::warn!("audio preload failed: {err:?}");
                        AUDIO.with(|a| *a.borrow_mut() = Phase::Idle);
                    }
                }
            });
        }
    }

    pub fn ready() -> bool {
        AUDIO.with(|a| matches!(&*a.borrow(), Phase::Ready(_)))
    }

    pub fn ensure_started() {
        preload(); // no-op if already past Idle
        AUDIO.with(|a| {
            let mut a = a.borrow_mut();
            match &mut *a {
                Phase::Ready(loaded) => {
                    if loaded.source.is_none() {
                        start_source(loaded);
                    } else {
                        // resume after tab-suspend
                        let _ = loaded.ctx.resume();
                    }
                }
                Phase::Loading { play_when_ready } => *play_when_ready = true,
                Phase::Idle => {}
            }
        });
    }

    async fn load() -> Result<Loaded, JsValue> {
        let window = web_sys::window().ok_or("no window")?;
        let resp: web_sys::Response = JsFuture::from(window.fetch_with_str(crate::vibe::TRACK_SRC))
            .await?
            .dyn_into()?;
        let bytes = JsFuture::from(resp.array_buffer()?).await?;

        let ctx = AudioContext::new()?;
        let decoded = JsFuture::from(ctx.decode_audio_data(&bytes.dyn_into()?)?).await?;
        let buffer: AudioBuffer = decoded.dyn_into()?;
        let duration = buffer.duration();

        let analyser = ctx.create_analyser()?;
        analyser.set_fft_size(1024);
        analyser.set_smoothing_time_constant(0.5);
        analyser.connect_with_audio_node(&ctx.destination().unchecked_into())?;

        let buf = vec![0u8; analyser.frequency_bin_count() as usize];
        Ok(Loaded {
            ctx,
            analyser,
            buffer,
            source: None,
            start_time: 0.0,
            duration,
            buf,
        })
    }

    fn start_source(l: &mut Loaded) {
        let _ = l.ctx.resume();
        if let Some(old) = l.source.take() {
            #[allow(deprecated)] // stop() is the correct API; web_sys mislabels it
            let _ = old.stop();
        }
        let Ok(source) = l.ctx.create_buffer_source() else {
            return;
        };
        source.set_buffer(Some(&l.buffer));
        source.set_loop(true);
        if source.connect_with_audio_node(&l.analyser).is_err() {
            return;
        }
        l.start_time = l.ctx.current_time();
        let _ = source.start();
        l.source = Some(source);
    }

    pub fn restart() {
        AUDIO.with(|a| {
            if let Phase::Ready(l) = &mut *a.borrow_mut() {
                start_source(l);
            }
        });
    }

    /// Returns (frequency bytes, playhead seconds) if audio is playing.
    pub fn sample() -> Option<(Vec<u8>, f64)> {
        AUDIO.with(|a| {
            let mut a = a.borrow_mut();
            let Phase::Ready(state) = &mut *a else {
                return None;
            };
            state.source.as_ref()?;
            state.analyser.get_byte_frequency_data(&mut state.buf);
            let t = (state.ctx.current_time() - state.start_time).rem_euclid(state.duration);
            Some((state.buf.clone(), t))
        })
    }

    pub fn save_calibration(offset_s: f64) {
        if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
            let _ = storage.set_item("mta_cal_offset", &offset_s.to_string());
        }
    }

    pub fn load_calibration() -> f64 {
        web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.get_item("mta_cal_offset").ok().flatten())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0)
    }
}
