//! Microphone capture as raw s16le mono audio at the rate a provider wants, in
//! 50 ms chunks, and the short sounds played when dictation starts and stops.

#[cfg(not(target_os = "linux"))]
mod cpal;
#[cfg(not(target_os = "linux"))]
pub use self::cpal::{Recorder, devices};
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::Recorder;
pub mod sound;

/// Loudness of a chunk of s16le samples, 0.0 (silence) to 1.0 (full scale),
/// on a log scale from -50 dBFS.
pub fn level(pcm: &[u8]) -> f32 {
    let n = pcm.len() / 2;
    if n == 0 {
        return 0.0;
    }
    let sum: f64 = pcm
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| {
            let s = i16::from_le_bytes(*b) as f64 / 32768.0;
            s * s
        })
        .sum();
    let rms = (sum / n as f64).sqrt();
    if rms <= 0.0 {
        return 0.0;
    }
    let db = 20.0 * rms.log10();
    ((db + 50.0) / 50.0).clamp(0.0, 1.0) as f32
}

/// Converts mono f32 audio from one sample rate to another, streaming. Each
/// output sample averages the input samples around it, which also filters out
/// what the lower rate can't hold. Plenty for speech recognition.
pub struct Resampler {
    ratio: f64,
    buf: Vec<f32>,
    /// Position in `buf` of the next output sample.
    t: f64,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        Self { ratio: from as f64 / to.max(1) as f64, buf: Vec::new(), t: 0.0 }
    }

    pub fn push(&mut self, input: &[f32], out: &mut Vec<i16>) {
        self.buf.extend_from_slice(input);
        let half = (self.ratio / 2.0).max(0.5);
        let len = self.buf.len() as f64;
        while self.t + half <= len {
            let lo = (self.t - half).ceil().max(0.0) as usize;
            let hi = ((self.t + half).floor() as usize).min(self.buf.len() - 1).max(lo);
            let win = &self.buf[lo..=hi];
            let avg = win.iter().sum::<f32>() / win.len() as f32;
            out.push((avg.clamp(-1.0, 1.0) * 32767.0) as i16);
            self.t += self.ratio;
        }
        let consumed = ((self.t - half).floor().max(0.0) as usize).min(self.buf.len());
        self.buf.drain(..consumed);
        self.t -= consumed as f64;
    }
}

/// Bytes in 50 ms of 16-bit mono audio.
pub fn chunk_bytes(rate: u32) -> usize {
    (rate as usize * 2 / 20) & !1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        assert_eq!(level(&[0; 320]), 0.0);
        let loud: Vec<u8> = (0..160).flat_map(|i| if i % 2 == 0 { 30000i16 } else { -30000 }.to_le_bytes()).collect();
        assert!(level(&loud) > 0.95);
        assert_eq!(chunk_bytes(16000), 1600);
        assert_eq!(chunk_bytes(24000), 2400);
    }

    #[test]
    fn resampling_keeps_level_and_length() {
        for (from, to) in [(48_000, 16_000), (44_100, 16_000), (16_000, 24_000), (48_000, 24_000)] {
            let mut r = Resampler::new(from, to);
            let mut out = Vec::new();
            // One second, fed in uneven pieces.
            let input = vec![0.5f32; from as usize];
            for piece in input.chunks(997) {
                r.push(piece, &mut out);
            }
            let want = to as i64;
            assert!((out.len() as i64 - want).abs() <= 2, "{from}->{to}: {} samples", out.len());
            assert!(out.iter().all(|&s| (s - 16383).abs() <= 1), "{from}->{to}: level changed");
        }
    }
}
