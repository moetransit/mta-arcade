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

/// Track clock for cosmetic beat pulses. `time_s` is the audio element's
/// playhead, judged against the track's analyzed beat grid.
#[derive(Resource, Default)]
pub struct BeatClock {
    pub beat_times: Vec<f64>,
    pub time_s: f64,
    pub playing: bool,
}

impl BeatClock {
    /// 0 at each beat, rising to 1 just before the next: a saw for pulses.
    pub fn beat_phase(&self) -> f32 {
        let i = self.beat_times.partition_point(|&b| b <= self.time_s);
        if i == 0 || i >= self.beat_times.len() {
            return 1.0;
        }
        let (prev, next) = (self.beat_times[i - 1], self.beat_times[i]);
        (((self.time_s - prev) / (next - prev)).clamp(0.0, 1.0)) as f32
    }
}

pub struct VibePlugin;

impl Plugin for VibePlugin {
    fn build(&self, app: &mut App) {
        let meta: serde_json::Value =
            serde_json::from_str(BEATS_JSON).expect("valid beats json");
        let beat_times = meta["beat_times"]
            .as_array()
            .expect("beat_times array")
            .iter()
            .filter_map(|v| v.as_f64())
            .collect();
        app.init_resource::<AudioBands>()
            .insert_resource(BeatClock {
                beat_times,
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
    use wasm_bindgen::JsCast;
    use web_sys::{AnalyserNode, AudioContext, HtmlAudioElement};

    struct AudioState {
        _ctx: AudioContext,
        analyser: AnalyserNode,
        element: HtmlAudioElement,
        buf: Vec<u8>,
    }

    thread_local! {
        static AUDIO: RefCell<Option<AudioState>> = const { RefCell::new(None) };
    }

    pub fn ensure_started() {
        AUDIO.with(|a| {
            let mut a = a.borrow_mut();
            if let Some(state) = a.as_ref() {
                // resume after tab-suspend; replay if the loop somehow stopped
                let _ = state.element.play();
                return;
            }
            match init() {
                Ok(state) => *a = Some(state),
                Err(err) => bevy::log::warn!("audio init failed: {err:?}"),
            }
        });
    }

    fn init() -> Result<AudioState, wasm_bindgen::JsValue> {
        let element = HtmlAudioElement::new_with_src(crate::vibe::TRACK_SRC)?;
        element.set_loop(true);
        let ctx = AudioContext::new()?;
        let source = ctx.create_media_element_source(&element)?;
        let analyser = ctx.create_analyser()?;
        analyser.set_fft_size(1024);
        analyser.set_smoothing_time_constant(0.5);
        source.connect_with_audio_node(&analyser)?;
        analyser.connect_with_audio_node(&ctx.destination().unchecked_into())?;
        let _ = element.play()?;
        let buf = vec![0u8; analyser.frequency_bin_count() as usize];
        Ok(AudioState {
            _ctx: ctx,
            analyser,
            element,
            buf,
        })
    }

    /// Returns (frequency bytes, playhead seconds) if audio is running.
    pub fn sample() -> Option<(Vec<u8>, f64)> {
        AUDIO.with(|a| {
            let mut a = a.borrow_mut();
            let state = a.as_mut()?;
            state.analyser.get_byte_frequency_data(&mut state.buf);
            Some((state.buf.clone(), state.element.current_time()))
        })
    }
}
