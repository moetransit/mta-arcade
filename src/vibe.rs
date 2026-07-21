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

/// Track clock for cosmetic beat pulses. `time_s` is the audio element's
/// playhead; the placeholder grid is a constant BPM from t=0.
#[derive(Resource)]
pub struct BeatClock {
    pub bpm: f32,
    pub time_s: f64,
    pub playing: bool,
}

impl Default for BeatClock {
    fn default() -> Self {
        Self {
            bpm: 140.0,
            time_s: 0.0,
            playing: false,
        }
    }
}

impl BeatClock {
    /// 0 at each beat, rising to 1 just before the next: a saw for pulses.
    pub fn beat_phase(&self) -> f32 {
        let spb = 60.0 / self.bpm as f64;
        ((self.time_s % spb) / spb) as f32
    }
}

pub struct VibePlugin;

impl Plugin for VibePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioBands>()
            .init_resource::<BeatClock>()
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
        let element = HtmlAudioElement::new_with_src("assets/tracks/placeholder_140.ogg")?;
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
