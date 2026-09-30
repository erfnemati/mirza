#!/bin/sh
# Installs Mirza from this folder for the current user (no root needed,
# except once for the virtual keyboard rule if it isn't allowed yet).
#   ./install.sh             install
#   ./install.sh --uninstall remove it again
set -eu

here=$(cd "$(dirname "$0")" && pwd)
bin_dir="$HOME/.local/bin"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
id=io.github.erfnemati.Mirza

if [ "${1:-}" = "--uninstall" ]; then
    "$bin_dir/mirza" quit 2>/dev/null || true
    rm -f "$bin_dir/mirza" "$bin_dir/mirza-panel" "$data/applications/$id.desktop" \
        "$data/applications/mirza-panel.desktop" "$data/icons/hicolor/scalable/apps/$id.svg" \
        "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/$id.desktop"
    echo "Mirza removed. Your settings are still in ~/.config/mirza."
    exit 0
fi

"$bin_dir/mirza" quit 2>/dev/null || true
install -Dm755 "$here/mirza" "$bin_dir/mirza"
install -Dm755 "$here/mirza-panel" "$bin_dir/mirza-panel"
install -Dm644 "$here/$id.desktop" "$data/applications/$id.desktop"
sed -i "s|^Exec=mirza|Exec=$bin_dir/mirza|" "$data/applications/$id.desktop"
install -Dm644 "$here/mirza-panel.desktop" "$data/applications/mirza-panel.desktop"
install -Dm644 "$here/$id.svg" "$data/icons/hicolor/scalable/apps/$id.svg"
touch "$data/icons/hicolor"
command -v update-desktop-database >/dev/null && update-desktop-database "$data/applications" 2>/dev/null || true
command -v kbuildsycoca6 >/dev/null && kbuildsycoca6 >/dev/null 2>&1 || true

if [ ! -w /dev/uinput ]; then
    echo "Mirza types with a virtual keyboard, which needs a one-time permission (sudo):"
    sudo install -Dm644 "$here/60-mirza-uinput.rules" /etc/udev/rules.d/60-mirza-uinput.rules
    sudo install -Dm644 "$here/mirza-uinput.conf" /etc/modules-load.d/mirza-uinput.conf
    sudo modprobe uinput || true
    sudo udevadm control --reload-rules || true
    sudo udevadm trigger --settle --name-match=/dev/uinput || true
    if [ ! -w /dev/uinput ]; then
        echo "Installed. Log out and back in, then start Mirza from your app menu."
        exit 0
    fi
fi

systemd-run --user --quiet --collect --unit="app-$id@$(date +%s)" "$bin_dir/mirza" >/dev/null 2>&1 ||
    (nohup "$bin_dir/mirza" >/dev/null 2>&1 &)
echo "Installed. Mirza is running in your tray."
