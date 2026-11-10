//! Interface sounds: synthesized in code (no audio files), played through cpal.
//!
//! # Sound rules
//! Every cue follows the same small set of rules so the interface sounds like one instrument:
//!
//! 1. **One key.** Every note comes from the D-major pentatonic scale (D E F# A B), so any
//!    cues that overlap are consonant.
//! 2. **One timbre.** A soft mallet: a sine with quiet 2nd/3rd harmonics that decay faster than
//!    the fundamental, a 3 ms raised-cosine attack and an exponential decay. No noise or
//!    clicks, and nothing below about 290 Hz, which small TV speakers can't reproduce.
//! 3. **Pitch means direction.** Going forward, opening something or turning it on rises;
//!    going back, closing or turning off falls. Moving focus plays the scale note for that
//!    position, so scrolling a row plays a gentle melody instead of the same tick.
//! 4. **Length and loudness mean weight.** Focus ticks are the shortest and quietest, then
//!    toggles and tabs, then selections and dialogs, then confirmations. Launching a game and
//!    the startup chime are the longest and fullest.
//! 5. **One gesture, one sound.** A cue that arrives within [`GESTURE_WINDOW`] of a
//!    higher-priority cue is dropped, and a higher-priority cue cuts the lower one short. A
//!    click that also moves focus and opens a dialog therefore plays only the dialog sound.
//!    Focus ticks are rate-limited so holding a direction doesn't pile up.
//! 6. **Blocked actions bump.** Pressing against an edge, or an action that does nothing,
//!    plays a short, dull low note instead of silence.

use std::collections::HashMap;
use std::f32::consts::TAU;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Sounds closer together than this count as one gesture (rule 5).
pub const GESTURE_WINDOW: Duration = Duration::from_millis(70);
/// Minimum spacing between focus ticks.
pub const TICK_SPACING: Duration = Duration::from_millis(28);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cue {
    /// Focus moved to position `n` (pitch walks the scale).
    Move(u8),
    /// Nothing happened: edge reached or action unavailable.
    Bump,
    /// Switched to tab `n`.
    Tab(u8),
    /// A setting stepped up (`true`) or down.
    Adjust(bool),
    ToggleOn,
    ToggleOff,
    Select,
    Back,
    Open,
    Close,
    Favorite,
    Unfavorite,
    Confirm,
    Error,
    Launch,
    /// Startup chime played under the loading animation.
    Boot,
}

const SCALE_STEPS: u8 = 5;
const TAB_STEPS: u8 = 7;

impl Cue {
    /// Maps the names used by the `Sfx.play(name, variant)` callback in the UI.
    pub fn from_name(name: &str, variant: i32) -> Option<Cue> {
        let n = variant.rem_euclid(256) as u8;
        Some(match name {
            "move" => Cue::Move(n % SCALE_STEPS),
            "bump" => Cue::Bump,
            "tab" => Cue::Tab(n % TAB_STEPS),
            "adjust" => Cue::Adjust(variant > 0),
            "toggle-on" => Cue::ToggleOn,
            "toggle-off" => Cue::ToggleOff,
            "select" => Cue::Select,
            "back" => Cue::Back,
            "open" => Cue::Open,
            "close" => Cue::Close,
            "favorite" => Cue::Favorite,
            "unfavorite" => Cue::Unfavorite,
            "confirm" => Cue::Confirm,
            "error" => Cue::Error,
            "launch" => Cue::Launch,
            "boot" => Cue::Boot,
            _ => return None,
        })
    }

    /// Every distinct cue, used to pre-render the bank.
    pub fn all() -> Vec<Cue> {
        let mut v: Vec<Cue> = (0..SCALE_STEPS).map(Cue::Move).collect();
        v.extend((0..TAB_STEPS).map(Cue::Tab));
        v.extend([
            Cue::Bump,
            Cue::Adjust(true),
            Cue::Adjust(false),
            Cue::ToggleOn,
            Cue::ToggleOff,
            Cue::Select,
            Cue::Back,
            Cue::Open,
            Cue::Close,
            Cue::Favorite,
            Cue::Unfavorite,
            Cue::Confirm,
            Cue::Error,
            Cue::Launch,
            Cue::Boot,
        ]);
        v
    }

    /// Rule 4/5: higher wins when cues collide.
    pub fn priority(self) -> u8 {
        match self {
            Cue::Move(_) | Cue::Bump | Cue::Adjust(_) => 1,
            Cue::Tab(_) | Cue::ToggleOn | Cue::ToggleOff | Cue::Close => 2,
            Cue::Select | Cue::Back | Cue::Open | Cue::Error => 3,
            Cue::Favorite | Cue::Unfavorite | Cue::Confirm => 4,
            Cue::Launch => 5,
            Cue::Boot => 6,
        }
    }

    /// Peak level before the user's volume (rule 4).
    fn peak(self) -> f32 {
        match self.priority() {
            1 => 0.24,
            2 => 0.30,
            3 => 0.34,
            4 => 0.38,
            5 => 0.44,
            _ => 0.48,
        }
    }

    fn is_tick(self) -> bool {
        matches!(self, Cue::Move(_) | Cue::Adjust(_))
    }

    /// Renders the cue as mono samples at `sr` Hz, normalised to its peak level.
    pub fn render(self, sr: u32) -> Vec<f32> {
        let mut b = Buf::new(sr);
        // Scale degrees in D major pentatonic, as MIDI notes.
        const D4: i32 = 62;
        const D5: i32 = 74;
        let deg = |octave_root: i32, step: i32| -> f32 {
            const P: [i32; 5] = [0, 2, 4, 7, 9];
            let o = step.div_euclid(5);
            midi(octave_root + 12 * o + P[step.rem_euclid(5) as usize])
        };
        match self {
            Cue::Move(i) => b.note(0.0, deg(D5, i as i32), 0.018, 1.0),
            Cue::Adjust(up) => b.note(0.0, deg(D5, if up { 4 } else { 2 }), 0.016, 1.0),
            Cue::Bump => {
                b.note_with(0.0, midi(D4), 0.030, 1.0, Tone::MUTED);
            }
            Cue::Tab(i) => {
                b.note(0.0, deg(D5, i as i32 - 2), 0.045, 1.0);
                b.note(0.0, deg(D5, i as i32 + 1), 0.035, 0.45);
            }
            Cue::ToggleOn => {
                b.note(0.0, deg(D5, 2), 0.030, 0.8);
                b.note(0.045, deg(D5, 3), 0.050, 1.0);
            }
            Cue::ToggleOff => {
                b.note(0.0, deg(D5, 3), 0.030, 0.8);
                b.note(0.045, deg(D5, 2), 0.050, 0.9);
            }
            Cue::Select => {
                b.note(0.0, deg(D5, 0), 0.040, 0.85);
                b.note(0.055, deg(D5, 3), 0.080, 1.0);
            }
            Cue::Back => {
                b.note(0.0, deg(D5, 3), 0.040, 0.85);
                b.note(0.055, deg(D5, 0), 0.070, 0.9);
            }
            Cue::Open => {
                for (k, s) in [0, 2, 3].into_iter().enumerate() {
                    b.note(k as f32 * 0.040, deg(D5, s), 0.090, 0.75 + 0.1 * k as f32);
                }
            }
            Cue::Close => {
                for (k, s) in [3, 2, 0].into_iter().enumerate() {
                    b.note(k as f32 * 0.035, deg(D5, s), 0.060, 0.95 - 0.1 * k as f32);
                }
            }
            Cue::Favorite => {
                for (k, s) in [4, 5, 7].into_iter().enumerate() {
                    b.note_with(k as f32 * 0.035, deg(D5, s), 0.140, 0.8, Tone::BRIGHT);
                }
            }
            Cue::Unfavorite => {
                b.note(0.0, deg(D5, 4), 0.050, 0.9);
                b.note(0.050, deg(D5, 2), 0.080, 0.8);
            }
            Cue::Confirm => {
                b.note(0.0, deg(D5, 0), 0.150, 0.8);
                b.note(0.0, deg(D5, 3), 0.150, 0.55);
                b.note(0.075, deg(D5, 5), 0.220, 1.0);
            }
            Cue::Error => {
                b.note_with(0.0, deg(D4, 1), 0.050, 0.9, Tone::MUTED);
                b.note_with(0.090, midi(D4), 0.080, 1.0, Tone::MUTED);
            }
            Cue::Launch => {
                for (k, s) in [0, 3, 5, 7, 8, 10].into_iter().enumerate() {
                    b.note(k as f32 * 0.045, deg(D4, s), 0.45, 0.55 + 0.06 * k as f32);
                }
                b.pad(
                    0.05,
                    1.2,
                    &[midi(D4), midi(D4 + 7), midi(D4 + 12)],
                    0.25,
                    0.35,
                );
                b.echo(0.17, 0.28, 3);
            }
            Cue::Boot => render_boot(&mut b),
        }
        b.finish(self.peak())
    }
}

/// The startup chime (about 3.4 s): a soft pad swells in, a rising pentatonic arpeggio
/// plays over it, and a bell chord resolves at about 1.9 s, which is when the loading
/// animation fills its bar.
fn render_boot(b: &mut Buf) {
    const D4: i32 = 62;
    b.pad(
        0.0,
        2.9,
        &[midi(D4), midi(69), midi(74), midi(78)], // D4 A4 D5 F#5
        0.7,
        0.9,
    );
    let arp = [69, 74, 76, 78, 81, 83, 86]; // A4 D5 E5 F#5 A5 B5 D6
    for (k, n) in arp.into_iter().enumerate() {
        b.note_with(
            0.30 + k as f32 * 0.17,
            midi(n),
            0.32,
            0.42 + 0.05 * k as f32,
            Tone::BELL,
        );
    }
    for (n, a) in [(74, 0.9), (81, 0.7), (86, 0.55), (90, 0.3)] {
        b.note_with(1.90, midi(n), 0.70, a, Tone::BELL);
    }
    b.echo(0.19, 0.30, 3);
    b.release(3.0, 3.6);
}

fn midi(n: i32) -> f32 {
    440.0 * 2f32.powf((n - 69) as f32 / 12.0)
}

/// Harmonic recipe for a note (rule 2: one timbre, three shades).
#[derive(Clone, Copy)]
struct Tone {
    h2: f32,
    h3: f32,
    attack: f32,
}

impl Tone {
    const SOFT: Tone = Tone {
        h2: 0.16,
        h3: 0.05,
        attack: 0.003,
    };
    const BRIGHT: Tone = Tone {
        h2: 0.24,
        h3: 0.10,
        attack: 0.002,
    };
    const BELL: Tone = Tone {
        h2: 0.20,
        h3: 0.08,
        attack: 0.004,
    };
    const MUTED: Tone = Tone {
        h2: 0.06,
        h3: 0.0,
        attack: 0.004,
    };
}

struct Buf {
    sr: f32,
    s: Vec<f32>,
}

impl Buf {
    fn new(sr: u32) -> Self {
        Self {
            sr: sr as f32,
            s: Vec::new(),
        }
    }

    fn ensure(&mut self, len: usize) {
        if self.s.len() < len {
            self.s.resize(len, 0.0);
        }
    }

    fn note(&mut self, at: f32, freq: f32, tau: f32, amp: f32) {
        self.note_with(at, freq, tau, amp, Tone::SOFT);
    }

    /// Mallet note: fundamental decays with time constant `tau`, harmonics decay 2x/3x faster.
    fn note_with(&mut self, at: f32, freq: f32, tau: f32, amp: f32, tone: Tone) {
        let start = (at * self.sr) as usize;
        let len = ((tone.attack + tau * 7.0) * self.sr) as usize;
        self.ensure(start + len);
        let att = tone.attack * self.sr;
        for i in 0..len {
            let t = i as f32 / self.sr;
            let a = if (i as f32) < att {
                0.5 - 0.5 * (std::f32::consts::PI * i as f32 / att).cos()
            } else {
                1.0
            };
            let ph = TAU * freq * t;
            let v = (-t / tau).exp() * ph.sin()
                + tone.h2 * (-2.0 * t / tau).exp() * (2.0 * ph).sin()
                + tone.h3 * (-3.0 * t / tau).exp() * (3.0 * ph).sin();
            self.s[start + i] += amp * a * v;
        }
    }

    /// Sustained pad: slightly detuned sine pairs with a slow raised-cosine swell and release.
    fn pad(&mut self, at: f32, dur: f32, freqs: &[f32], swell: f32, amp: f32) {
        let start = (at * self.sr) as usize;
        let len = (dur * self.sr) as usize;
        self.ensure(start + len);
        let per = amp / freqs.len() as f32;
        let release = (dur * 0.4).min(1.2);
        for i in 0..len {
            let t = i as f32 / self.sr;
            let env = if t < swell {
                0.5 - 0.5 * (std::f32::consts::PI * t / swell).cos()
            } else if t > dur - release {
                let r = (dur - t) / release;
                0.5 - 0.5 * (std::f32::consts::PI * r).cos()
            } else {
                1.0
            };
            let mut v = 0.0;
            for &f in freqs {
                v += (TAU * f * 0.9985 * t).sin() + (TAU * f * 1.0015 * t).sin();
            }
            self.s[start + i] += per * 0.5 * env * v;
        }
    }

    /// Feedback-free multi-tap echo: `taps` copies `delay` apart, each `fb` quieter.
    fn echo(&mut self, delay: f32, fb: f32, taps: usize) {
        let d = (delay * self.sr) as usize;
        let dry = self.s.clone();
        self.ensure(dry.len() + d * taps);
        let mut g = 1.0;
        for k in 1..=taps {
            g *= fb;
            for (i, v) in dry.iter().enumerate() {
                self.s[i + d * k] += v * g;
            }
        }
    }

    /// Raised-cosine fade from `from` to `to` seconds, then cut: keeps long cues to length.
    fn release(&mut self, from: f32, to: f32) {
        let (a, b) = ((from * self.sr) as usize, (to * self.sr) as usize);
        self.s.truncate(b);
        for i in a.min(self.s.len())..self.s.len() {
            let r = (i - a) as f32 / (b - a) as f32;
            self.s[i] *= 0.5 + 0.5 * (std::f32::consts::PI * r).cos();
        }
    }

    /// Trims silence, adds a short tail fade so the cue never ends on a click, and scales the
    /// loudest sample to `peak`.
    fn finish(mut self, peak: f32) -> Vec<f32> {
        let floor = 1e-4 * self.s.iter().fold(0f32, |m, v| m.max(v.abs()));
        while self.s.last().is_some_and(|v| v.abs() <= floor) {
            self.s.pop();
        }
        let fade = ((0.012 * self.sr) as usize).min(self.s.len());
        let n = self.s.len();
        for i in 0..fade {
            self.s[n - fade + i] *= 1.0 - (i + 1) as f32 / fade as f32;
        }
        let max = self.s.iter().fold(0f32, |m, v| m.max(v.abs()));
        if max > 0.0 {
            let k = peak / max;
            self.s.iter_mut().for_each(|v| *v *= k);
        }
        self.s
    }
}

/// Rule 5 bookkeeping on the UI side: decides whether a cue plays and what it cuts.
#[derive(Debug, Default)]
pub struct Gate {
    recent: Vec<(Instant, u8)>,
    last_tick: Option<Instant>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum GateDecision {
    Drop,
    /// Play, fading out any voice of this priority or lower that started in the window.
    Play {
        cut_up_to: u8,
    },
}

impl Gate {
    pub fn decide(&mut self, cue: Cue, now: Instant) -> GateDecision {
        self.recent
            .retain(|(t, _)| now.saturating_duration_since(*t) < GESTURE_WINDOW);
        let p = cue.priority();
        if self.recent.iter().any(|&(_, q)| q > p) {
            return GateDecision::Drop;
        }
        if cue.is_tick() {
            if let Some(t) = self.last_tick {
                if now.saturating_duration_since(t) < TICK_SPACING {
                    return GateDecision::Drop;
                }
            }
            self.last_tick = Some(now);
        }
        let cut = if self.recent.is_empty() { 0 } else { p };
        self.recent.retain(|&(_, q)| q > p);
        self.recent.push((now, p));
        GateDecision::Play { cut_up_to: cut }
    }
}

enum Command {
    Play { cue: Cue, gain: f32, cut_up_to: u8 },
}

/// Handle to the audio thread. If no output device exists every call is a silent no-op.
pub struct SoundEngine {
    tx: Option<Sender<Command>>,
    gate: std::cell::RefCell<Gate>,
}

impl SoundEngine {
    /// Opens the default output device on a dedicated thread and pre-renders every cue.
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("kiln-audio".into())
            .spawn(move || audio_thread(rx, ready_tx));
        let ok = spawned.is_ok()
            && ready_rx
                .recv_timeout(Duration::from_secs(3))
                .unwrap_or(false);
        if !ok {
            eprintln!("Sound: no audio output available; interface sounds are off");
        }
        Self {
            tx: ok.then_some(tx),
            gate: Default::default(),
        }
    }

    /// A silent engine (tests, or when audio is unavailable).
    pub fn silent() -> Self {
        Self {
            tx: None,
            gate: Default::default(),
        }
    }

    /// Plays `cue` at `volume` (0..1, perceptual). Applies the gesture rules.
    pub fn play(&self, cue: Cue, volume: f32) {
        let Some(tx) = &self.tx else { return };
        let volume = volume.clamp(0.0, 1.0);
        if volume <= 0.0 {
            return;
        }
        if let GateDecision::Play { cut_up_to } = self.gate.borrow_mut().decide(cue, Instant::now())
        {
            let _ = tx.send(Command::Play {
                cue,
                gain: volume.powf(1.5),
                cut_up_to,
            });
        }
    }
}

struct Voice {
    samples: Arc<[f32]>,
    pos: usize,
    gain: f32,
    priority: u8,
    /// Remaining fade-out samples when cut short.
    fade: Option<usize>,
}

fn audio_thread(rx: Receiver<Command>, ready: Sender<bool>) {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        let _ = ready.send(false);
        return;
    };
    let Ok(supported) = device.default_output_config() else {
        let _ = ready.send(false);
        return;
    };
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.config();
    let sr = config.sample_rate;
    let channels = config.channels as usize;

    let bank: HashMap<Cue, Arc<[f32]>> = Cue::all()
        .into_iter()
        .map(|c| (c, Arc::from(c.render(sr))))
        .collect();
    let mut mixer = Mixer {
        bank,
        voices: Vec::new(),
        rx,
        window: (GESTURE_WINDOW.as_secs_f32() * sr as f32) as usize,
        fade_len: (0.006 * sr as f32) as usize,
    };

    let err = |e: cpal::Error| eprintln!("Sound: stream error: {e}");
    let stream = match format {
        cpal::SampleFormat::F32 => device.build_output_stream::<f32, _, _>(
            config,
            move |out, _| mixer.fill(out, channels, |v| v),
            err,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream::<i16, _, _>(
            config,
            move |out, _| mixer.fill(out, channels, |v| (v * i16::MAX as f32) as i16),
            err,
            None,
        ),
        cpal::SampleFormat::I32 => device.build_output_stream::<i32, _, _>(
            config,
            move |out, _| mixer.fill(out, channels, |v| (v as f64 * i32::MAX as f64) as i32),
            err,
            None,
        ),
        other => {
            eprintln!("Sound: unsupported output format {other}");
            let _ = ready.send(false);
            return;
        }
    };
    let Ok(stream) = stream else {
        let _ = ready.send(false);
        return;
    };
    if stream.play().is_err() {
        let _ = ready.send(false);
        return;
    }
    let _ = ready.send(true);
    // The stream lives as long as this thread; park it for the life of the process.
    loop {
        std::thread::park();
    }
}

struct Mixer {
    bank: HashMap<Cue, Arc<[f32]>>,
    voices: Vec<Voice>,
    rx: Receiver<Command>,
    window: usize,
    fade_len: usize,
}

impl Mixer {
    fn fill<T: Copy>(&mut self, out: &mut [T], channels: usize, conv: impl Fn(f32) -> T) {
        while let Ok(Command::Play {
            cue,
            gain,
            cut_up_to,
        }) = self.rx.try_recv()
        {
            for v in &mut self.voices {
                if v.priority <= cut_up_to && v.pos < self.window && v.fade.is_none() {
                    v.fade = Some(self.fade_len);
                }
            }
            if let Some(s) = self.bank.get(&cue) {
                self.voices.push(Voice {
                    samples: s.clone(),
                    pos: 0,
                    gain,
                    priority: cue.priority(),
                    fade: None,
                });
            }
        }
        let fade_len = self.fade_len.max(1) as f32;
        for frame in out.chunks_mut(channels.max(1)) {
            let mut x = 0.0f32;
            for v in &mut self.voices {
                if let Some(s) = v.samples.get(v.pos) {
                    let f = match &mut v.fade {
                        Some(r) => {
                            *r = r.saturating_sub(1);
                            *r as f32 / fade_len
                        }
                        None => 1.0,
                    };
                    x += s * v.gain * f;
                }
                v.pos += 1;
            }
            // Gentle limiter: transparent below 0.6, never exceeds 1.0.
            let y = if x.abs() <= 0.6 {
                x
            } else {
                x.signum() * (0.6 + 0.4 * ((x.abs() - 0.6) / 0.4).tanh())
            };
            let s = conv(y);
            frame.iter_mut().for_each(|o| *o = s);
        }
        self.voices
            .retain(|v| v.pos < v.samples.len() && v.fade != Some(0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: u32 = 48_000;

    #[test]
    fn every_cue_renders_cleanly() {
        for cue in Cue::all() {
            let s = cue.render(SR);
            assert!(!s.is_empty(), "{cue:?} is empty");
            let peak = s.iter().fold(0f32, |m, v| m.max(v.abs()));
            assert!((peak - cue.peak()).abs() < 1e-3, "{cue:?} peak {peak}");
            assert!(s.iter().all(|v| v.is_finite()));
            // Starts and ends at (near) silence: no clicks.
            assert!(s[0].abs() < 0.01, "{cue:?} starts at {}", s[0]);
            assert!(s.last().unwrap().abs() < 0.01, "{cue:?} ends with a click");
        }
    }

    #[test]
    fn length_follows_weight() {
        let secs = |c: Cue| c.render(SR).len() as f32 / SR as f32;
        assert!(secs(Cue::Move(0)) < 0.2);
        assert!(secs(Cue::Bump) < 0.25);
        assert!(secs(Cue::Select) < secs(Cue::Confirm));
        assert!(secs(Cue::Confirm) < secs(Cue::Launch));
        let boot = secs(Cue::Boot);
        assert!((2.8..4.5).contains(&boot), "boot is {boot}s");
    }

    #[test]
    fn move_ticks_vary_by_position() {
        let a = Cue::Move(0).render(SR);
        let b = Cue::Move(3).render(SR);
        assert_ne!(a.len(), 0);
        assert_ne!(a, b);
        assert_eq!(Cue::from_name("move", 8), Some(Cue::Move(3)));
        assert_eq!(Cue::from_name("move", -1), Some(Cue::Move(0)));
        assert_eq!(Cue::from_name("nope", 0), None);
    }

    #[test]
    fn gate_one_gesture_one_sound() {
        let mut g = Gate::default();
        let t0 = Instant::now();
        assert_eq!(
            g.decide(Cue::Move(0), t0),
            GateDecision::Play { cut_up_to: 0 }
        );
        // Dialog opens in the same gesture: plays and cuts the tick.
        assert_eq!(
            g.decide(Cue::Open, t0 + Duration::from_millis(5)),
            GateDecision::Play { cut_up_to: 3 }
        );
        // A lower cue in the same gesture is dropped.
        assert_eq!(
            g.decide(Cue::Close, t0 + Duration::from_millis(10)),
            GateDecision::Drop
        );
        // After the window, anything plays again.
        assert_eq!(
            g.decide(Cue::Move(1), t0 + Duration::from_millis(200)),
            GateDecision::Play { cut_up_to: 0 }
        );
        // Ticks closer than the spacing are dropped.
        assert_eq!(
            g.decide(Cue::Move(2), t0 + Duration::from_millis(210)),
            GateDecision::Drop
        );
    }

    #[test]
    fn mixer_cuts_and_limits() {
        let (tx, rx) = mpsc::channel();
        let mut m = Mixer {
            bank: Cue::all()
                .into_iter()
                .map(|c| (c, Arc::from(c.render(SR))))
                .collect(),
            voices: Vec::new(),
            rx,
            window: 48_000,
            fade_len: 288,
        };
        tx.send(Command::Play {
            cue: Cue::Move(0),
            gain: 1.0,
            cut_up_to: 0,
        })
        .unwrap();
        let mut out = vec![0f32; 2 * 256];
        m.fill(&mut out, 2, |v| v);
        assert_eq!(m.voices.len(), 1);
        tx.send(Command::Play {
            cue: Cue::Launch,
            gain: 4.0,
            cut_up_to: 5,
        })
        .unwrap();
        let mut out = vec![0f32; 2 * 4800];
        m.fill(&mut out, 2, |v| v);
        assert_eq!(m.voices.len(), 1, "tick was cut");
        assert!(out.iter().all(|v| v.abs() <= 1.0));
        assert!(out.chunks(2).all(|f| f[0] == f[1]));
    }

    /// `KILN_SFX_DIR=<dir> cargo test -p kiln-shell export_wavs` writes every cue as a WAV
    /// file so the set can be auditioned outside the app.
    #[test]
    fn export_wavs() {
        let Ok(dir) = std::env::var("KILN_SFX_DIR") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for cue in Cue::all() {
            let s = cue.render(SR);
            let mut w = Vec::with_capacity(44 + s.len() * 2);
            let data = (s.len() * 2) as u32;
            w.extend_from_slice(b"RIFF");
            w.extend_from_slice(&(36 + data).to_le_bytes());
            w.extend_from_slice(b"WAVEfmt ");
            w.extend_from_slice(&16u32.to_le_bytes());
            w.extend_from_slice(&1u16.to_le_bytes());
            w.extend_from_slice(&1u16.to_le_bytes());
            w.extend_from_slice(&SR.to_le_bytes());
            w.extend_from_slice(&(SR * 2).to_le_bytes());
            w.extend_from_slice(&2u16.to_le_bytes());
            w.extend_from_slice(&16u16.to_le_bytes());
            w.extend_from_slice(b"data");
            w.extend_from_slice(&data.to_le_bytes());
            for v in s {
                w.extend_from_slice(&((v * 32767.0) as i16).to_le_bytes());
            }
            let name = format!("{cue:?}").replace(['(', ')'], "").to_lowercase();
            std::fs::write(dir.join(format!("{name}.wav")), w).unwrap();
        }
    }
}
