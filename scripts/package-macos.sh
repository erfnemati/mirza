#!/bin/sh
# Builds Mirza.app (Apple Silicon + Intel) and a .dmg. Run on macOS.
#
# Signing: set MACOS_SIGN_IDENTITY to a code-signing identity in your keychain
# (a self-signed one is fine). Keeping the same identity across versions keeps
# the Accessibility and Microphone permissions when users update. Without it
# the app is signed ad hoc, and macOS asks for the permissions again after
# each update.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
dist="$root/dist"
app="$dist/Mirza.app"

for target in aarch64-apple-darwin x86_64-apple-darwin; do
    rustup target add "$target" >/dev/null 2>&1 || true
    cargo build --release --target "$target" -p mirza -p mirza-panel
done

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
for bin in mirza mirza-panel; do
    lipo -create -output "$app/Contents/MacOS/$bin" \
        "target/aarch64-apple-darwin/release/$bin" "target/x86_64-apple-darwin/release/$bin"
done
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"

iconset="$dist/mirza.iconset"
rm -rf "$iconset" && mkdir -p "$iconset"
for s in 16 32 128 256 512; do
    cp "assets/icons/png/mirza-$s.png" "$iconset/icon_${s}x${s}.png"
    cp "assets/icons/png/mirza-$((s * 2)).png" "$iconset/icon_${s}x${s}@2x.png" 2>/dev/null ||
        sips -z $((s * 2)) $((s * 2)) "assets/icons/png/mirza-1024.png" --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/mirza.icns"
rm -rf "$iconset"

identity="${MACOS_SIGN_IDENTITY:--}"
codesign --force --deep --entitlements packaging/macos/entitlements.plist --sign "$identity" "$app"

stage="$dist/dmg"
rm -rf "$stage" && mkdir -p "$stage"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
hdiutil create -volname Mirza -srcfolder "$stage" -ov -format UDZO "$dist/Mirza-$version-macos-universal.dmg"
rm -rf "$stage"
echo "Built $dist/Mirza-$version-macos-universal.dmg"
