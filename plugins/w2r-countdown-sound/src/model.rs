//! Pure rules: when to beep, and the beeps themselves.

pub const KEY: &str = "volume";
pub const DEFAULT_VOLUME: i32 = 60;
pub const MAX_VOLUME: i32 = 100;

/// The setting as the host's options screen shows it (`settings_declare`): a slider next to the
/// game's own volume settings.
pub const CATEGORY: &str = "Sound";
pub const SUBCATEGORY: &str = "Countdown";
pub const LABEL: &str = "Countdown Volume";
pub const DESCRIPTION: &str = "Volume of the beeps for each second of the lobby start countdown, played while the game is in the background. 0 is silent.";

/// Bytes from the countdown flag to the `u32` holding the seconds left (0x95d430 - 0x95d426).
pub const SECONDS_OFFSET: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    /// A second of the countdown ran down.
    Tick,
    /// The countdown reached zero and the game is starting.
    Go,
}

/// Turns the game's (flag, seconds left) samples into cues. Fed on every poll.
#[derive(Default)]
pub struct Tracker {
    active: bool,
    seconds: u32,
}

impl Tracker {
    pub fn step(&mut self, active: bool, seconds: u32) -> Option<Cue> {
        let cue = if active && (!self.active || seconds != self.seconds) {
            Some(Cue::Tick)
        } else if !active && self.active && self.seconds <= 1 {
            // The game clears the flag in the frame the last second ends. A cancelled start or a
            // left lobby clears it with seconds still left.
            Some(Cue::Go)
        } else {
            None
        };
        self.active = active;
        if active {
            self.seconds = seconds;
        }
        cue
    }
}

pub const SAMPLE_RATE: u32 = 22_050;

/// Frequency and length of a cue.
pub fn tone(cue: Cue) -> (f32, u32) {
    match cue {
        Cue::Tick => (880.0, 120),
        Cue::Go => (1320.0, 450),
    }
}

fn header(out: &mut Vec<u8>, channels: u16, rate: u32, bits: u16, data_len: u32) {
    let align = channels * (bits / 8);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * align as u32).to_le_bytes());
    out.extend_from_slice(&align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
}

/// A mono 16-bit PCM WAV file of a sine tone with a short fade in and out (no clicks).
/// `volume` is 0..=100 and is clamped.
pub fn wav(freq: f32, ms: u32, volume: i32) -> Vec<u8> {
    let n = (SAMPLE_RATE as u64 * ms as u64 / 1000) as usize;
    let gain = volume.clamp(0, 100) as f32 / 100.0 * 0.6 * i16::MAX as f32;
    let fade = (SAMPLE_RATE as usize / 200).min(n / 2).max(1);
    let mut out = Vec::with_capacity(44 + n * 2);
    header(&mut out, 1, SAMPLE_RATE, 16, (n * 2) as u32);
    for i in 0..n {
        let edge = i.min(n - 1 - i);
        let env = if edge < fade { edge as f32 / fade as f32 } else { 1.0 };
        let t = i as f32 / SAMPLE_RATE as f32;
        let v = (t * freq * core::f32::consts::TAU).sin() * env * gain;
        out.extend_from_slice(&(v as i16).to_le_bytes());
    }
    out
}

/// The PCM fields of a `WAVEFORMATEX` (its first 16 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PcmFormat {
    pub tag: u16,
    pub channels: u16,
    pub rate: u32,
    pub bits: u16,
}

impl PcmFormat {
    pub fn parse(b: &[u8]) -> Option<PcmFormat> {
        if b.len() < 16 {
            return None;
        }
        let u16_at = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
        Some(PcmFormat {
            tag: u16_at(0),
            channels: u16_at(2),
            rate: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
            bits: u16_at(14),
        })
    }

    /// Plain integer PCM that `wav_from_pcm` can scale.
    pub fn usable(&self) -> bool {
        self.tag == 1 && (1..=2).contains(&self.channels) && matches!(self.bits, 8 | 16) && self.rate > 0
    }
}

/// The game's own sound as a WAV file, scaled by `volume` (0..=100, 100 is as recorded).
pub fn wav_from_pcm(f: &PcmFormat, data: &[u8], volume: i32) -> Option<Vec<u8>> {
    if !f.usable() {
        return None;
    }
    let gain = volume.clamp(0, 100) as f32 / 100.0;
    let frame = (f.channels * f.bits / 8) as usize;
    let data = &data[..data.len() / frame * frame];
    let mut out = Vec::with_capacity(44 + data.len());
    header(&mut out, f.channels, f.rate, f.bits, data.len() as u32);
    if f.bits == 16 {
        for c in data.chunks_exact(2) {
            let v = i16::from_le_bytes([c[0], c[1]]) as f32 * gain;
            out.extend_from_slice(&(v as i16).to_le_bytes());
        }
    } else {
        // 8-bit PCM is unsigned around 128.
        out.extend(data.iter().map(|&u| ((u as f32 - 128.0) * gain + 128.0) as u8));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_declaration_fits_the_hosts_limits() {
        assert!(LABEL.len() <= 64 && DESCRIPTION.len() <= 256);
        assert!(CATEGORY.len() <= 32 && SUBCATEGORY.len() <= 32);
        for text in [LABEL, DESCRIPTION, CATEGORY, SUBCATEGORY] {
            assert!(text.is_ascii() && !text.is_empty());
        }
        assert!((0..=MAX_VOLUME).contains(&DEFAULT_VOLUME));
    }

    fn run(samples: &[(bool, u32)]) -> Vec<Option<Cue>> {
        let mut t = Tracker::default();
        samples.iter().map(|&(a, s)| t.step(a, s)).collect()
    }

    #[test]
    fn a_full_countdown_ticks_each_second_then_goes() {
        let cues = run(&[
            (false, 0),
            (true, 5),
            (true, 5),
            (true, 4),
            (true, 3),
            (true, 3),
            (true, 2),
            (true, 1),
            (false, 0),
            (false, 0),
        ]);
        use Cue::*;
        assert_eq!(
            cues,
            [None, Some(Tick), None, Some(Tick), Some(Tick), None, Some(Tick), Some(Tick), Some(Go), None]
        );
    }

    #[test]
    fn a_cancelled_countdown_does_not_go() {
        let cues = run(&[(true, 5), (true, 4), (false, 4), (false, 0)]);
        assert_eq!(cues, [Some(Cue::Tick), Some(Cue::Tick), None, None]);
    }

    #[test]
    fn a_new_countdown_after_a_cancel_ticks_again() {
        let cues = run(&[(true, 4), (false, 4), (true, 4)]);
        assert_eq!(cues, [Some(Cue::Tick), None, Some(Cue::Tick)]);
    }

    #[test]
    fn a_countdown_already_running_when_we_start_watching_ticks_once() {
        assert_eq!(run(&[(true, 3), (true, 3)]), [Some(Cue::Tick), None]);
    }

    #[test]
    fn wav_header_and_length_are_consistent() {
        let (f, ms) = tone(Cue::Tick);
        let w = wav(f, ms, 60);
        let n = (SAMPLE_RATE as usize * ms as usize) / 1000;
        assert_eq!(w.len(), 44 + n * 2);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()) as usize, w.len() - 8);
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()) as usize, n * 2);
    }

    #[test]
    fn volume_scales_and_zero_is_silent() {
        let peak = |v| {
            wav(880.0, 120, v)[44..]
                .chunks(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]).unsigned_abs())
                .max()
                .unwrap()
        };
        assert_eq!(peak(0), 0);
        assert!(peak(100) > peak(50) && peak(50) > 0);
        assert_eq!(peak(500), peak(100));
    }

    #[test]
    fn the_tone_starts_and_ends_at_zero() {
        let w = wav(880.0, 120, 100);
        assert_eq!(&w[44..46], &[0, 0]);
        let last = &w[w.len() - 2..];
        assert!(i16::from_le_bytes([last[0], last[1]]).unsigned_abs() < 2000);
    }

    #[test]
    fn the_game_sound_is_rewrapped_and_scaled() {
        let mut fmt = vec![0u8; 16];
        fmt[0..2].copy_from_slice(&1u16.to_le_bytes());
        fmt[2..4].copy_from_slice(&1u16.to_le_bytes());
        fmt[4..8].copy_from_slice(&22_050u32.to_le_bytes());
        fmt[14..16].copy_from_slice(&16u16.to_le_bytes());
        let f = PcmFormat::parse(&fmt).unwrap();
        let data: Vec<u8> = [1000i16, -1000, 7].iter().flat_map(|v| v.to_le_bytes()).collect();
        let w = wav_from_pcm(&f, &data, 50).unwrap();
        assert_eq!(w.len(), 44 + 6);
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 22_050);
        assert_eq!(i16::from_le_bytes([w[44], w[45]]), 500);
        assert_eq!(i16::from_le_bytes([w[46], w[47]]), -500);
        // A trailing partial frame is dropped.
        assert_eq!(wav_from_pcm(&f, &data[..5], 100).unwrap().len(), 44 + 4);
    }

    #[test]
    fn eight_bit_sound_scales_around_the_midpoint() {
        let f = PcmFormat { tag: 1, channels: 1, rate: 11_025, bits: 8 };
        let w = wav_from_pcm(&f, &[128, 228, 28], 50).unwrap();
        assert_eq!(&w[44..], &[128, 178, 78]);
    }

    #[test]
    fn unusable_formats_are_refused() {
        let adpcm = PcmFormat { tag: 2, channels: 1, rate: 22_050, bits: 4 };
        assert!(wav_from_pcm(&adpcm, &[0; 8], 100).is_none());
        let float = PcmFormat { tag: 3, channels: 1, rate: 22_050, bits: 32 };
        assert!(!float.usable());
        assert!(PcmFormat::parse(&[0; 15]).is_none());
    }
}
