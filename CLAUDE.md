# Mirza

Voice typing for any app: a small Rust tray app that streams the microphone to a speech-to-text
service (Soniox, OpenAI, ElevenLabs) and types the text into the focused window, on Linux, Windows
and macOS. Persian and English in the same sentence must keep working (half-spaces, Persian letters).
What comes next is in [docs/roadmap.md](docs/roadmap.md).

## Layout

- `crates/core`: settings (`config.rs`), API keys (`secrets.rs`), providers (`providers/`), proxy and
  WebSocket (`net.rs`), audio capture, usage, update check. Platform-independent; the Android app
  will reuse it.
- `crates/hotkey`, `crates/inject`: global shortcuts and typing, per OS.
- `crates/daemon`: the tray app (`mirza` binary).
- `crates/panel`: the settings window (Tauri, plain JS in `ui/`). Needs WebKitGTK to build on Linux.

## Build and test

Same as CI (`.github/workflows/ci.yml`):

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Try a provider with a recorded clip (needs ffmpeg and a key in `~/.config/mirza/keys.toml`, e.g.
`soniox = "..."`, file mode 600):

```sh
cargo run -p mirza-core --example transcribe -- soniox clip.wav
```

The development server has no real microphone and no display, but it has KVM. Two scripts stand
in:

- `scripts/dev/virtual-mic.sh start`, then `play clip.m4a`: audio files come out of the default
  microphone, for Mirza's Linux recorder and for the emulator.
- `scripts/dev/emulator.sh start | wait | mic | screen F.png | stop`: an Android 16 emulator (SDK in
  `~/Android/Sdk`) on a virtual screen. With KVM a cold boot takes about 2 minutes (snapshots
  stay off); stop it when you're done, the server is shared. Without KVM the script falls back to
  software emulation, which takes 7 to 30 minutes to boot and whose microphone goes silent. Run
  `mic` before each recording: the emulator turns its host microphone off by itself. In tests,
  `uiautomator dump` pauses accessibility services, so Mirza's typing stops while it runs.

Shortcuts, typing into desktop windows and the tray still need a real desktop, and the emulator
says nothing about how the app feels on a real phone: say what was tested where. Desktop builds for manual
tests come from the Release workflow run by hand (workflow artifacts).

## Rules

- Git: `develop` collects the work, feature branches come from it (`feature/...`, `fix/...`) and
  go back through a PR. `main` holds releases only.
- CI minutes: run the three checks above locally first. Open PRs as drafts (CI skips them) and mark
  them ready once, after merging the latest `develop`. Pushing a branch runs no CI; only Rust
  changes do.
- Release: bump `version` in the root `Cargo.toml`, merge `develop` into `main`, tag `vX.Y.Z`; the
  Release workflow builds the installers. Only when Erfan asks.
- No AI attribution in commits or PRs (no Co-Authored-By trailer). Older commits have one; don't
  copy it.
- Every network call goes through the proxy setting (`net::connect_ws`, `usage::http_client`).
  Many users are where these services are blocked.
- Error messages are plain words and say what usually helps (see `providers/mod.rs`).
- Never log or print API keys or the recognized text, except with `MIRZA_LOG=mirza=debug`.
- Config keys are strict (`deny_unknown_fields`): a new key needs a default, and older versions
  can't read a config that has it.
