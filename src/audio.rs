//! Sounds, all synthesized with Web Audio (no files to download or cache):
//! end-of-session alarms, a ticking clock and looping background noise.
//! One shared `AudioContext`, created on the first user gesture.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use wasm_bindgen::JsCast;
use web_sys::{AudioBufferSourceNode, AudioContext, GainNode, OscillatorType};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Alarm {
    None,
    #[default]
    Beep,
    Bell,
    Digital,
    Soft,
}

impl Alarm {
    pub const ALL: [Alarm; 5] = [Alarm::None, Alarm::Beep, Alarm::Bell, Alarm::Digital, Alarm::Soft];

    pub fn id(self) -> &'static str {
        match self {
            Alarm::None => "none",
            Alarm::Beep => "beep",
            Alarm::Bell => "bell",
            Alarm::Digital => "digital",
            Alarm::Soft => "soft",
        }
    }

    pub fn from_id(id: &str) -> Alarm {
        Alarm::ALL.into_iter().find(|a| a.id() == id).unwrap_or_default()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Noise {
    #[default]
    None,
    White,
    Pink,
    Brown,
}

impl Noise {
    pub const ALL: [Noise; 4] = [Noise::None, Noise::White, Noise::Pink, Noise::Brown];

    pub fn id(self) -> &'static str {
        match self {
            Noise::None => "none",
            Noise::White => "white",
            Noise::Pink => "pink",
            Noise::Brown => "brown",
        }
    }

    pub fn from_id(id: &str) -> Noise {
        Noise::ALL.into_iter().find(|n| n.id() == id).unwrap_or_default()
    }
}

struct NoisePlayer {
    kind: Noise,
    source: AudioBufferSourceNode,
    gain: GainNode,
}

thread_local! {
    static CTX: RefCell<Option<AudioContext>> = const { RefCell::new(None) };
    static NOISE: RefCell<Option<NoisePlayer>> = const { RefCell::new(None) };
}

/// Create (or resume) the shared context. Call from a click handler:
/// browsers keep audio suspended until a user gesture.
pub fn unlock() {
    let _ = ctx();
}

fn ctx() -> Option<AudioContext> {
    CTX.with(|c| {
        let mut c = c.borrow_mut();
        if c.is_none() {
            *c = AudioContext::new().ok();
        }
        let ctx = c.clone()?;
        if ctx.state() == web_sys::AudioContextState::Suspended {
            let _ = ctx.resume();
        }
        Some(ctx)
    })
}

/// One enveloped oscillator note: quick attack, exponential decay.
fn note(ctx: &AudioContext, kind: OscillatorType, freq: f32, start: f64, dur: f64, peak: f32) {
    let (Ok(osc), Ok(gain)) = (ctx.create_oscillator(), ctx.create_gain()) else { return };
    osc.set_type(kind);
    osc.frequency().set_value(freq);
    let g = gain.gain();
    let _ = g.set_value_at_time(0.0001, start);
    let _ = g.exponential_ramp_to_value_at_time(peak.max(0.0001), start + 0.01);
    let _ = g.exponential_ramp_to_value_at_time(0.0001, start + dur);
    let _ = osc.connect_with_audio_node(&gain);
    let _ = gain.connect_with_audio_node(&ctx.destination());
    let _ = osc.start_with_when(start);
    let _ = osc.stop_with_when(start + dur + 0.05);
}

/// `volume` is 0–100.
pub fn play_alarm(alarm: Alarm, volume: u32) {
    if alarm == Alarm::None || volume == 0 {
        return;
    }
    let Some(ctx) = ctx() else { return };
    let v = volume.min(100) as f32 / 100.0;
    let t = ctx.current_time() + 0.02;
    match alarm {
        Alarm::None => {}
        Alarm::Beep => {
            for i in 0..3 {
                note(&ctx, OscillatorType::Sine, 880.0, t + i as f64 * 0.3, 0.2, 0.35 * v);
            }
        }
        Alarm::Bell => {
            for i in 0..2 {
                let s = t + i as f64 * 1.2;
                note(&ctx, OscillatorType::Sine, 660.0, s, 1.6, 0.35 * v);
                note(&ctx, OscillatorType::Sine, 1320.0, s, 1.0, 0.12 * v);
                note(&ctx, OscillatorType::Sine, 1980.0, s, 0.6, 0.06 * v);
            }
        }
        Alarm::Digital => {
            for group in 0..3 {
                for i in 0..4 {
                    let s = t + group as f64 * 0.6 + i as f64 * 0.1;
                    note(&ctx, OscillatorType::Square, 1000.0, s, 0.06, 0.12 * v);
                }
            }
        }
        Alarm::Soft => {
            for (i, f) in [523.25, 659.25, 783.99, 1046.5].into_iter().enumerate() {
                note(&ctx, OscillatorType::Triangle, f, t + i as f64 * 0.22, 0.9, 0.3 * v);
            }
        }
    }
}

/// A short tick; `tock` alternates the pitch.
pub fn tick(tock: bool, volume: u32) {
    if volume == 0 {
        return;
    }
    let Some(ctx) = ctx() else { return };
    let v = volume.min(100) as f32 / 100.0;
    let freq = if tock { 1300.0 } else { 1700.0 };
    note(&ctx, OscillatorType::Square, freq, ctx.current_time() + 0.005, 0.03, 0.05 * v);
}

fn noise_buffer(ctx: &AudioContext, kind: Noise) -> Option<web_sys::AudioBuffer> {
    let rate = ctx.sample_rate();
    let len = (rate * 4.0) as u32;
    let buffer = ctx.create_buffer(1, len, rate).ok()?;
    let mut data = vec![0f32; len as usize];
    let mut rng: u32 = 0x9e37_79b9;
    let mut white = move || {
        // xorshift32: plenty for audio noise, no extra dependency.
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        (rng as f32 / u32::MAX as f32) * 2.0 - 1.0
    };
    match kind {
        Noise::None => return None,
        Noise::White => data.iter_mut().for_each(|s| *s = white() * 0.5),
        Noise::Pink => {
            // Paul Kellet's refined pink filter.
            let (mut b0, mut b1, mut b2, mut b3, mut b4, mut b5, mut b6) = (0f32, 0f32, 0f32, 0f32, 0f32, 0f32, 0f32);
            for s in data.iter_mut() {
                let w = white();
                b0 = 0.99886 * b0 + w * 0.0555179;
                b1 = 0.99332 * b1 + w * 0.0750759;
                b2 = 0.96900 * b2 + w * 0.1538520;
                b3 = 0.86650 * b3 + w * 0.3104856;
                b4 = 0.55000 * b4 + w * 0.5329522;
                b5 = -0.7616 * b5 - w * 0.0168980;
                *s = (b0 + b1 + b2 + b3 + b4 + b5 + b6 + w * 0.5362) * 0.11;
                b6 = w * 0.115926;
            }
        }
        Noise::Brown => {
            let mut last = 0f32;
            for s in data.iter_mut() {
                last = (last + 0.02 * white()) / 1.02;
                *s = last * 3.5;
            }
        }
    }
    buffer.copy_to_channel(&data, 0).ok()?;
    Some(buffer)
}

/// Play `kind` in a loop at `volume` (0–100), or stop with `Noise::None`.
/// Calling again with the same kind only adjusts the volume.
pub fn set_noise(kind: Noise, volume: u32) {
    NOISE.with(|n| {
        let mut n = n.borrow_mut();
        let gain_value = volume.min(100) as f32 / 100.0 * 0.6;
        if let Some(p) = n.as_ref() {
            if p.kind == kind && kind != Noise::None {
                p.gain.gain().set_value(gain_value);
                return;
            }
        }
        if let Some(p) = n.take() {
            let _ = p.source.unchecked_ref::<web_sys::AudioScheduledSourceNode>().stop();
            let _ = p.source.disconnect();
        }
        if kind == Noise::None || volume == 0 {
            return;
        }
        let Some(ctx) = ctx() else { return };
        let Some(buffer) = noise_buffer(&ctx, kind) else { return };
        let (Ok(source), Ok(gain)) = (ctx.create_buffer_source(), ctx.create_gain()) else { return };
        source.set_buffer(Some(&buffer));
        source.set_loop(true);
        gain.gain().set_value(gain_value);
        let _ = source.connect_with_audio_node(&gain);
        let _ = gain.connect_with_audio_node(&ctx.destination());
        let _ = source.unchecked_ref::<web_sys::AudioScheduledSourceNode>().start();
        *n = Some(NoisePlayer { kind, source, gain });
    });
}
