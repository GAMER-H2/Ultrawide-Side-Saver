#!/usr/bin/env bash
# Remove everything install.sh created. Leaves the config file alone.
set -euo pipefail

BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
APP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
AUTOSTART_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"
CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}/ultrawide-side-saver/config.toml"

"$BIN_DIR/ultrawide-side-saver" quit 2>/dev/null || true

rm -fv "$BIN_DIR/ultrawide-side-saver" \
       "$APP_DIR/ultrawide-side-saver.desktop" \
       "$APP_DIR/ultrawide-side-saver-toggle.desktop" \
       "$AUTOSTART_DIR/ultrawide-side-saver.desktop"

update-desktop-database "$APP_DIR" 2>/dev/null || true
kbuildsycoca6 --noincremental >/dev/null 2>&1 || true

kwriteconfig6 --file kglobalshortcutsrc \
  --group services --group ultrawide-side-saver-toggle.desktop \
  --key _launch --delete 2>/dev/null || true

echo
echo "Removed. Your config is still at:"
echo "  $CONFIG"
