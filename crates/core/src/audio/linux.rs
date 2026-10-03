//! Recording through PipeWire or PulseAudio's command-line recorders
//! (parecord, pw-record, or arecord for plain ALSA), which resample to the
//! requested rate for us.

use std::io::{self, Read};
use std::process::{Child, Command, Stdio};
use std::thread::JoinHandle;

use tokio::sync::mpsc;

use super::{chunk_bytes, level};

pub struct Recorder {
    child: Child,
    reader: Option<JoinHandle<()>>,
}

impl Recorder {
    /// Starts recording from `device` (empty: the default microphone). Audio
    /// goes to `tx` in 50 ms chunks until the recorder stops; then the
    /// channel closes. `on_level` gets each chunk's loudness.
    pub fn start(
        custom_cmd: &str,
        device: &str,
        rate: u32,
        tx: mpsc::Sender<Vec<u8>>,
        on_level: impl Fn(f32) + Send + 'static,
    ) -> io::Result<Self> {
        let args = command(custom_cmd, device, rate)?;
        let mut child = Command::new(&args[0])
            .args(&args[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| io::Error::new(e.kind(), format!("starting {}: {e}", args[0])))?;
        let mut out = child.stdout.take().expect("piped stdout");
        let size = chunk_bytes(rate);
        let reader = std::thread::Builder::new().name("mirza-audio".into()).spawn(move || {
            loop {
                let mut buf = vec![0u8; size];
                let mut n = 0;
                while n < size {
                    match out.read(&mut buf[n..]) {
                        Ok(0) | Err(_) => break,
                        Ok(k) => n += k,
                    }
                }
                buf.truncate(n & !1); // whole samples only
                if buf.is_empty() {
                    return;
                }
                on_level(level(&buf));
                let last = n < size;
                if tx.blocking_send(buf).is_err() || last {
                    return;
                }
            }
        })?;
        Ok(Self { child, reader: Some(reader) })
    }

    /// Asks the recorder to stop; the audio channel closes once the rest is read.
    pub fn stop(&self) {
        // SAFETY: sending a signal to our own child process.
        unsafe { libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM) };
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
    }
}

fn command(custom: &str, device: &str, rate: u32) -> io::Result<Vec<String>> {
    if !custom.trim().is_empty() {
        let rate = rate.to_string();
        return Ok(custom.split_whitespace().map(|a| a.replace("{rate}", &rate).replace("{device}", device)).collect());
    }
    let rate = rate.to_string();
    let mut args: Vec<String>;
    if has_command("parecord") {
        args = ["parecord", "--raw", "--format=s16le", "--channels=1", "--latency-msec=30", "--client-name=Mirza"]
            .map(String::from)
            .to_vec();
        args.push(format!("--rate={rate}"));
        if !device.is_empty() {
            args.push(format!("--device={device}"));
        }
    } else if has_command("pw-record") {
        args = ["pw-record", "--raw", "--channels", "1", "--format", "s16", "--latency", "30ms"]
            .map(String::from)
            .to_vec();
        args.extend(["--rate".into(), rate]);
        if !device.is_empty() {
            args.push(format!("--target={device}"));
        }
        args.push("-".into());
    } else if has_command("arecord") {
        args = ["arecord", "-q", "-t", "raw", "-f", "S16_LE", "-c", "1"].map(String::from).to_vec();
        args.extend(["-r".into(), rate]);
        if !device.is_empty() {
            args.extend(["-D".into(), device.into()]);
        }
    } else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no audio recorder found: install pulseaudio-utils (parecord), pipewire-bin (pw-record) or alsa-utils (arecord)",
        ));
    }
    Ok(args)
}

pub(super) fn has_command(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
}
