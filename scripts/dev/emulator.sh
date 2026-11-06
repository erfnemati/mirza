#!/bin/sh
# The Android emulator on the dev server, which has no display.
# It runs on a virtual screen (Xvfb) because the headless build has no audio.
# With KVM (/dev/kvm, nested virtualization on the VM) a cold boot takes about
# 2 minutes. Without it, it falls back to software CPU emulation: 7 to 30 minutes,
# and its microphone goes silent after a second. Snapshots are off: without KVM
# a saved one fails to load ("Length mismatch" on a RAM block), and /home is small.
# Its microphone is the default PulseAudio source: run scripts/dev/virtual-mic.sh start first.
# The emulator still hears only silence until its host microphone is switched on (`mic`), and
# it switches it off again by itself (seen after reinstalling the app): run `mic` before each test.
# It refuses to start when /home is nearly full (it did with 1.6 GB free, not with 2.6 GB).
#
#   scripts/dev/emulator.sh start        boot in the background
#   scripts/dev/emulator.sh wait         wait until Android has booted (up to 45 min)
#   scripts/dev/emulator.sh mic          switch the host microphone on
#   scripts/dev/emulator.sh screen F.png save a screenshot
#   scripts/dev/emulator.sh stop         stop it
#
# MIRZA_AVD (default mirza-test) and MIRZA_EMU_PORT (default 5584) pick another one.
set -eu
SDK=${ANDROID_HOME:-$HOME/Android/Sdk}
AVD=${MIRZA_AVD:-mirza-test}
PORT=${MIRZA_EMU_PORT:-5584}
IMAGE="system-images;android-36;google_apis;x86_64"
ADB="$SDK/platform-tools/adb"
DEV="emulator-$PORT"
LOG="${XDG_RUNTIME_DIR:-/tmp}/mirza-emulator-$PORT.log"

case "${1:-}" in
start)
  if ! "$SDK/emulator/emulator" -list-avds | grep -qx "$AVD"; then
    echo no | "$SDK/cmdline-tools/latest/bin/avdmanager" create avd -n "$AVD" -k "$IMAGE" -d pixel_6 >/dev/null
  fi
  if [ -w /dev/kvm ]; then
    accel="-accel on -memory 3072"
  elif [ -e /dev/kvm ] && id -nG "$(id -un)" | grep -qw kvm; then
    # In the kvm group, but this shell started before joining it: run again with the group.
    exec sg kvm -c "\"$0\" start"
  else
    # Software emulation runs every virtual CPU on one host thread: more cores only add load.
    accel="-accel off -cores 2 -memory 3072"
  fi
  "$ADB" start-server >/dev/null 2>&1
  # $accel is split into words on purpose.
  PULSE_SERVER="unix:${XDG_RUNTIME_DIR:-/run/user/$(id -u)}/pulse/native" \
    nohup nice -n 15 xvfb-run -a -s "-screen 0 1280x1024x24" "$SDK/emulator/emulator" -avd "$AVD" \
    -port "$PORT" -no-boot-anim -no-metrics -gpu swiftshader_indirect $accel -allow-host-audio -no-snapshot \
    >"$LOG" 2>&1 &
  echo "starting $AVD as $DEV (log: $LOG)"
  ;;
wait)
  start=$(date +%s)
  until [ "$("$ADB" -s "$DEV" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]; do
    [ $(($(date +%s) - start)) -lt 2700 ] || { echo "not booted after 45 min, see $LOG" >&2; exit 1; }
    sleep 15
  done
  echo "booted after $(($(date +%s) - start)) s"
  ;;
mic)
  "$ADB" -s "$DEV" emu avd hostmicon >/dev/null
  ;;
screen)
  "$ADB" -s "$DEV" exec-out screencap -p >"${2:?usage: emulator.sh screen <file.png>}"
  ;;
stop)
  "$ADB" -s "$DEV" emu kill
  ;;
*)
  echo "usage: $0 start | wait | mic | screen <file.png> | stop" >&2
  exit 2
  ;;
esac
