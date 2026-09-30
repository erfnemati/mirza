//! Recording on Windows and macOS through cpal (WASAPI, CoreAudio). The
//! microphone's own format is converted to mono and resampled to the rate the
//! provider wants.

use std::io;
use std::sync::mpsc as std_mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tokio::sync::mpsc;

use super::{Resampler, chunk_bytes, level};

pub struct Recorder {
    stop: Option<std_mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Recorder {
    /// Starts recording from the input device named `device` (empty: the
    /// default). `custom_cmd` is only used on Linux.
    pub fn start(
        _custom_cmd: &str,
        device: &str,
        rate: u32,
        tx: mpsc::Sender<Vec<u8>>,
        on_level: impl Fn(f32) + Send + 'static,
    ) -> io::Result<Self> {
        let (ready_tx, ready_rx) = std_mpsc::channel::<Result<(), String>>();
        let (stop_tx, stop_rx) = std_mpsc::channel::<()>();
        let device = device.to_owned();
        // The stream lives on its own thread: some backends tie it to the
        // thread that made it.
        let thread = std::thread::Builder::new().name("mirza-audio".into()).spawn(move || {
            let (samples_tx, samples_rx) = std_mpsc::channel::<Vec<f32>>();
            let (stream, in_rate) = match open(&device, samples_tx) {
                Ok(s) => s,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return;
                }
            };
            let _ = ready_tx.send(Ok(()));
            let mut resampler = Resampler::new(in_rate, rate);
            let size = chunk_bytes(rate);
            let mut pcm: Vec<u8> = Vec::with_capacity(size * 2);
            let mut out: Vec<i16> = Vec::new();
            let mut stopping = false;
            loop {
                if !stopping && stop_rx.try_recv().is_ok() {
                    stopping = true;
                    drop(stream.pause());
                }
                let got = samples_rx.recv_timeout(Duration::from_millis(20));
                match got {
                    Ok(buf) => {
                        out.clear();
                        resampler.push(&buf, &mut out);
                        pcm.extend(out.iter().flat_map(|s| s.to_le_bytes()));
                        while pcm.len() >= size {
                            let chunk: Vec<u8> = pcm.drain(..size).collect();
                            on_level(level(&chunk));
                            if tx.blocking_send(chunk).is_err() {
                                return;
                            }
                        }
                    }
                    Err(std_mpsc::RecvTimeoutError::Timeout) if stopping => break,
                    Err(std_mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std_mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            if !pcm.is_empty() {
                let _ = tx.blocking_send(pcm);
            }
            drop(stream);
            // Dropping tx closes the audio channel: the provider sends the rest.
        })?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self { stop: Some(stop_tx), thread: Some(thread) }),
            Ok(Err(e)) => Err(io::Error::other(e)),
            Err(_) => Err(io::Error::other("the audio thread stopped")),
        }
    }

    /// Stops recording; the audio channel closes once the rest is delivered.
    pub fn stop(&self) {
        if let Some(s) = &self.stop {
            let _ = s.send(());
        }
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop();
        self.stop.take();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Opens the device and starts a stream that sends mono f32 samples.
fn open(device: &str, samples: std_mpsc::Sender<Vec<f32>>) -> Result<(cpal::Stream, u32), String> {
    let host = cpal::default_host();
    let dev = if device.is_empty() {
        host.default_input_device()
    } else {
        host.input_devices()
            .map_err(|e| e.to_string())?
            .find(|d| d.to_string() == device)
            .or_else(|| host.default_input_device())
    }
    .ok_or("no microphone found")?;
    let supported = dev.default_input_config().map_err(|e| format!("microphone: {e}"))?;
    let channels = supported.channels() as usize;
    let rate = supported.sample_rate();
    let config = supported.config();
    let err = |e: cpal::Error| tracing::warn!("microphone: {e}");
    macro_rules! build {
        ($t:ty, $to_f32:expr) => {{
            let samples = samples.clone();
            dev.build_input_stream::<$t, _, _>(
                config.clone(),
                move |data: &[$t], _| {
                    let mono: Vec<f32> = data
                        .chunks(channels.max(1))
                        .map(|frame| frame.iter().map(|s| $to_f32(*s)).sum::<f32>() / frame.len() as f32)
                        .collect();
                    let _ = samples.send(mono);
                },
                err,
                None,
            )
        }};
    }
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build!(f32, |s: f32| s),
        cpal::SampleFormat::I16 => build!(i16, |s: i16| s as f32 / 32768.0),
        cpal::SampleFormat::U16 => build!(u16, |s: u16| (s as f32 - 32768.0) / 32768.0),
        cpal::SampleFormat::I32 => build!(i32, |s: i32| s as f32 / 2_147_483_648.0),
        other => return Err(format!("unsupported microphone format {other:?}")),
    }
    .map_err(|e| format!("microphone: {e}"))?;
    stream.play().map_err(|e| format!("microphone: {e}"))?;
    Ok((stream, rate))
}

/// Input devices by name, for the settings window.
pub fn devices() -> Vec<String> {
    cpal::default_host().input_devices().map(|it| it.map(|d| d.to_string()).collect()).unwrap_or_default()
}
