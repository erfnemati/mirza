#!/bin/sh
# Builds Mirza and installs it for the current user (no root needed, except
# once for the uinput rule if the virtual keyboard isn't allowed yet).
#
#   scripts/install-linux.sh           build and install to ~/.local
#   scripts/install-linux.sh --no-build install the existing release build
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
bin_dir="$HOME/.local/bin"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
id=io.github.erfnemati.Mirza

if [ "${1:-}" != "--no-build" ]; then
    PATH="$HOME/.cargo/bin:$PATH" cargo build --release --manifest-path "$root/Cargo.toml" -p mirza -p mirza-panel
fi

# Stop a running copy so the binary can be replaced.
"$bin_dir/mirza" quit 2>/dev/null || true

install -Dm755 "$root/target/release/mirza" "$bin_dir/mirza"
if [ -f "$root/target/release/mirza-panel" ]; then
    install -Dm755 "$root/target/release/mirza-panel" "$bin_dir/mirza-panel"
fi
install -Dm644 "$root/packaging/linux/$id.desktop" "$data/applications/$id.desktop"
install -Dm644 "$root/packaging/linux/mirza-panel.desktop" "$data/applications/mirza-panel.desktop"
sed -i "s|^Exec=mirza|Exec=$bin_dir/mirza|" "$data/applications/$id.desktop"
for icon in "$root"/assets/icons/*.svg; do
    install -Dm644 "$icon" "$data/icons/hicolor/scalable/apps/$(basename "$icon")"
done
# Tell icon caches the icons changed (KDE and GTK both cache them).
touch "$data/icons/hicolor"
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true
rm -f "${XDG_CACHE_HOME:-$HOME/.cache}/icon-cache.kcache"
command -v kbuildsycoca6 >/dev/null && kbuildsycoca6 >/dev/null 2>&1 || true
command -v update-desktop-database >/dev/null && update-desktop-database "$data/applications" 2>/dev/null || true

if [ ! -w /dev/uinput ]; then
    echo "Mirza needs permission to create a virtual keyboard. Installing the uinput rule (sudo):"
    sudo install -Dm644 "$root/packaging/linux/60-mirza-uinput.rules" /etc/udev/rules.d/60-mirza-uinput.rules
    sudo install -Dm644 "$root/packaging/linux/mirza-uinput.conf" /etc/modules-load.d/mirza-uinput.conf
    sudo sh "$root/packaging/linux/deb/postinst" configure
    if [ ! -w /dev/uinput ]; then
        echo "Log out and back in, then start Mirza from the app menu."
        exit 0
    fi
fi

# Start it the way the desktop starts apps: as its own user service, so it has
# its own app identity (the shortcuts portal names Mirza's shortcuts by it)
# and keeps running after this terminal closes.
systemd-run --user --quiet --collect --unit="app-$id@$(date +%s)" "$bin_dir/mirza" >/dev/null 2>&1 || true
sleep 2
echo "Installed. Mirza is running in the tray; settings: $("$bin_dir/mirza" config-path)"
