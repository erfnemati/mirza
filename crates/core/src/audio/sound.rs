//! The short sounds played when dictation starts and stops. They are made in
//! code, so there are no sound files to ship, and played with the system's
//! own player without waiting for them to end.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// Two notes going up: listening.
    Start,
    /// Two notes going down: stopped listening.
    Stop,
    /// Two low notes: something went wrong.
    Error,
}

const RATE: u32 = 24_000;
/// Length of one note in seconds.
const NOTE: f32 = 0.12;

impl Sound {
    /// Each note's pitch in Hz and start time in seconds.
    fn notes(self) -> [(f32, f32); 2] {
        match self {
            Sound::Start => [(784.0, 0.0), (1046.5, 0.07)],
            Sound::Stop => [(1046.5, 0.0), (784.0, 0.07)],
            Sound::Error => [(330.0, 0.0), (330.0, 0.13)],
        }
    }
}

/// Plays a sound at `volume` (0 to 100) and returns right away.
pub fn play(sound: Sound, volume: u8) {
    if volume == 0 {
        return;
    }
    play_wav(sound, volume, wav(sound, volume));
}

/// The sound as a 16-bit mono WAV file.
pub fn wav(sound: Sound, volume: u8) -> Vec<u8> {
    let notes = sound.notes();
    let len = notes.iter().map(|n| n.1).fold(0.0, f32::max) + NOTE;
    let mut buf = vec![0f32; (len * RATE as f32) as usize];
    for (freq, at) in notes {
        let start = (at * RATE as f32) as usize;
        for i in 0..(NOTE * RATE as f32) as usize {
            let t = i as f32 / RATE as f32;
            // A soft bell: quick attack, quick decay, a little of the octave
            // above, and a short fade so it ends without a click.
            let env = (t / 0.004).min(1.0) * (-t / 0.035).exp() * ((NOTE - t) / 0.01).min(1.0);
            let w = std::f32::consts::TAU * freq * t;
            if let Some(s) = buf.get_mut(start + i) {
                *s += env * (w.sin() + 0.25 * (2.0 * w).sin());
            }
        }
    }
    let peak = buf.iter().fold(0f32, |m, s| m.max(s.abs())).max(1e-6);
    // Loudness is heard on a log scale; squaring keeps low settings usable.
    let v = volume.min(100) as f32 / 100.0;
    let gain = 0.8 * v * v / peak;

    let data_len = (buf.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + buf.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // format chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes()); // bytes per second
    out.extend_from_slice(&2u16.to_le_bytes()); // bytes per frame
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in buf {
        out.extend_from_slice(&(((s * gain).clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
    }
    out
}

/// Linux: PulseAudio's, PipeWire's or ALSA's player, reading from stdin.
#[cfg(target_os = "linux")]
fn play_wav(sound: Sound, _volume: u8, wav: Vec<u8>) {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let _ = std::thread::Builder::new().name("mirza-sound".into()).spawn(move || {
        // Played as Mirza's own stream, not as a notification sound: the sound
        // server remembers one volume per role, and when notification sounds
        // are muted or at zero, Mirza's would be silent too. This way they
        // follow Mirza's own volume setting, and show up as "Mirza" in the
        // system's mixer.
        let (prog, args): (&str, &[&str]) = if super::linux::has_command("paplay") {
            ("paplay", &["--client-name=Mirza", "--stream-name=Mirza", "--latency-msec=30"])
        } else if super::linux::has_command("pw-play") {
            ("pw-play", &["-P", "{ application.name = Mirza }", "-"])
        } else if super::linux::has_command("aplay") {
            ("aplay", &["-q", "-"])
        } else {
            return;
        };
        let child =
            Command::new(prog).args(args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
        match child {
            Ok(mut c) => {
                if let Some(mut stdin) = c.stdin.take() {
                    let _ = stdin.write_all(&wav);
                }
                let status = c.wait();
                tracing::debug!("{sound:?} sound played with {prog}: {status:?}");
            }
            Err(e) => tracing::debug!("playing a sound with {prog}: {e}"),
        }
    });
}

/// macOS: afplay, which needs a file.
#[cfg(target_os = "macos")]
fn play_wav(sound: Sound, volume: u8, wav: Vec<u8>) {
    let _ = std::thread::Builder::new().name("mirza-sound".into()).spawn(move || {
        let name = format!("mirza-{}-{sound:?}-{volume}.wav", env!("CARGO_PKG_VERSION")).to_lowercase();
        let path = std::env::temp_dir().join(name);
        if !path.is_file() {
            let tmp = path.with_extension("tmp");
            if let Err(e) = std::fs::write(&tmp, &wav).and_then(|_| std::fs::rename(&tmp, &path)) {
                tracing::debug!("writing a sound file: {e}");
                return;
            }
        }
        if let Err(e) = std::process::Command::new("afplay").arg(&path).status() {
            tracing::debug!("playing a sound with afplay: {e}");
        }
    });
}

/// Windows: PlaySound, straight from memory.
#[cfg(windows)]
fn play_wav(_sound: Sound, _volume: u8, wav: Vec<u8>) {
    use std::sync::Mutex;
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};

    // Windows reads the sound from this buffer while it plays, so it stays
    // here until the next sound has stopped this one.
    static PLAYING: Mutex<Vec<u8>> = Mutex::new(Vec::new());
    let mut current = PLAYING.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: a null sound stops whatever is playing, so the old buffer is no
    // longer read.
    unsafe { PlaySoundW(std::ptr::null(), std::ptr::null_mut(), 0) };
    *current = wav;
    // SAFETY: with SND_MEMORY the first argument is the WAV data, which stays
    // alive in PLAYING until the next call has stopped it.
    unsafe { PlaySoundW(current.as_ptr().cast(), std::ptr::null_mut(), SND_MEMORY | SND_ASYNC | SND_NODEFAULT) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_valid_wav() {
        for s in [Sound::Start, Sound::Stop, Sound::Error] {
            let w = wav(s, 50);
            assert_eq!(&w[0..4], b"RIFF");
            assert_eq!(&w[36..40], b"data");
            let data = u32::from_le_bytes(w[40..44].try_into().unwrap()) as usize;
            assert_eq!(w.len(), 44 + data, "{s:?}");
            assert!(data / 2 > RATE as usize / 10, "{s:?} is at least 0.1 s");
            let peak = w[44..].as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b).unsigned_abs()).max().unwrap();
            assert!((6000..=7000).contains(&peak), "{s:?} at half volume peaks at 0.2: {peak}");
            let last = i16::from_le_bytes([w[w.len() - 2], w[w.len() - 1]]);
            assert_eq!(last, 0, "{s:?} ends in silence");
        }
        let loud = wav(Sound::Start, 100);
        let peak = loud[44..].as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b).unsigned_abs()).max().unwrap();
        assert!(peak > 26000);
    }
}
