#!/bin/sh
# Builds mirza-mobile (crates/mobile) for Android with cargo-ndk and writes its
# Kotlin bindings. Gradle runs this before every build; cargo only rebuilds what
# changed.
#
#   build-rust.sh <dev|release> <jniLibs dir> <bindings dir> <abi>...
#
# Needs Rust with the Android targets (rustup target add aarch64-linux-android
# x86_64-linux-android), cargo-ndk, and the NDK (ANDROID_NDK_HOME, or the
# newest one under $ANDROID_HOME/ndk).
set -eu
[ $# -ge 4 ] || { echo "usage: $0 <dev|release> <jniLibs dir> <bindings dir> <abi>..." >&2; exit 2; }
profile=$1 jni=$2 out=$3
shift 3
PATH="$HOME/.cargo/bin:$PATH"
export ANDROID_HOME="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}"
cd "$(dirname "$0")/.."

targets=""
for abi in "$@"; do targets="$targets -t $abi"; done
release=""
[ "$profile" = release ] && release=--release
# -P: the app's minSdk.
# shellcheck disable=SC2086
cargo ndk $targets -P 29 -o "$jni" build $release -p mirza-mobile

# The bindings come from the library's UniFFI metadata, which the stripped
# release builds above don't keep, so they are read from a host build. They
# are the same for every platform.
cargo build -q -p mirza-mobile --features bindgen --lib
host_lib=$(ls target/debug/libmirza_mobile.so target/debug/libmirza_mobile.dylib target/debug/mirza_mobile.dll 2>/dev/null | head -n 1)
rm -rf "$out"
cargo run -q -p mirza-mobile --features bindgen --bin uniffi-bindgen -- \
  generate --library "$host_lib" --language kotlin --out-dir "$out" --no-format
