#!/bin/sh
# A virtual microphone for a machine without one, such as the dev server.
# Audio files played into it come out of the default microphone, for Mirza on
# Linux (parecord) and for the Android emulator (-allow-host-audio).
#
#   scripts/dev/virtual-mic.sh start           start PulseAudio with the virtual mic
#   scripts/dev/virtual-mic.sh play clip.m4a   "speak" a clip in real time (any format ffmpeg reads)
#   scripts/dev/virtual-mic.sh stop            stop PulseAudio
#
# Sounds Mirza plays go to a silent speaker, so they never leak into the mic.
set -eu
FEED=mirza_mic_feed      # what is played here...
MIC=mirza_mic            # ...is recorded from here
SPEAKER=mirza_speaker    # where other sounds go

has() { pactl list short "$1" | cut -f2 | grep -qx "$2"; }

case "${1:-}" in
start)
  pulseaudio --check 2>/dev/null || pulseaudio --start --exit-idle-time=-1 --log-target=syslog
  has sinks "$FEED" || pactl load-module module-null-sink sink_name=$FEED \
    sink_properties=device.description=Mirza-mic-feed >/dev/null
  has sinks "$SPEAKER" || pactl load-module module-null-sink sink_name=$SPEAKER \
    sink_properties=device.description=Mirza-speaker >/dev/null
  has sources "$MIC" || pactl load-module module-remap-source master=$FEED.monitor source_name=$MIC \
    source_properties=device.description=Mirza-virtual-mic >/dev/null
  # A real mic sends silence between words. An idle null sink sends almost
  # nothing, so keep silence playing into the feed; clips mix on top of it.
  pactl unload-module module-suspend-on-idle 2>/dev/null || true
  if ! pactl list short sink-inputs | grep -q "mirza-silence"; then
    nohup pacat --device=$FEED --format=s16le --channels=1 --rate=48000 \
      --client-name=mirza-silence --stream-name=mirza-silence </dev/zero >/dev/null 2>&1 &
  fi
  pactl set-default-sink $SPEAKER
  pactl set-default-source $MIC
  echo "virtual mic ready (default source: $MIC)"
  ;;
play)
  f=${2:?usage: virtual-mic.sh play <audio file>}
  ffmpeg -loglevel error -i "$f" -f s16le -ac 1 -ar 48000 - |
    pacat --device=$FEED --format=s16le --channels=1 --rate=48000 --latency-msec=50
  ;;
stop)
  pulseaudio --kill 2>/dev/null || true
  echo "stopped"
  ;;
*)
  echo "usage: $0 start | play <audio file> | stop" >&2
  exit 2
  ;;
esac
