#!/usr/bin/env bash
# Install ultrawide-side-saver for the current user (no root needed).
#
#   ./install.sh                    # build, install, bind Meta+Shift+B
#   ./install.sh 'Meta+F11'         # pick a different shortcut
#   SKIP_BUILD=1 ./install.sh       # reuse an existing release build
#
# Note: do NOT use Ctrl+Alt+F1..F12. The XKB "CTRL+ALT" key type maps that level
# to XF86Switch_VT_n, and KWin consumes it to switch virtual terminal.
set -euo pipefail

SHORTCUT="${1:-Meta+Shift+B}"
SRC_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
APP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
AUTOSTART_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/autostart"

BIN="$BIN_DIR/ultrawide-side-saver"
TOGGLE_DESKTOP_ID="ultrawide-side-saver-toggle.desktop"

say() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m warn:\033[0m %s\n' "$*" >&2; }

# 1. Build -------------------------------------------------------------------
if [[ -z "${SKIP_BUILD:-}" ]]; then
  say "Building release binary"
  ( cd "$SRC_DIR" && cargo build --release )
fi
[[ -x "$SRC_DIR/target/release/ultrawide-side-saver" ]] \
  || { echo "no release binary; run without SKIP_BUILD" >&2; exit 1; }

# 2. Binary ------------------------------------------------------------------
say "Installing $BIN"
install -Dm755 "$SRC_DIR/target/release/ultrawide-side-saver" "$BIN"

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) warn "$BIN_DIR is not on your PATH; the shortcut still works (it uses the full path)" ;;
esac

# 3. Command-shortcut launcher ----------------------------------------------
# Plasma 6 has no khotkeys: a "command shortcut" is a hidden .desktop file
# flagged with X-KDE-GlobalAccel-CommandShortcut, bound via kglobalshortcutsrc.
say "Installing $APP_DIR/$TOGGLE_DESKTOP_ID"
mkdir -p "$APP_DIR"
cat > "$APP_DIR/$TOGGLE_DESKTOP_ID" <<EOF
[Desktop Entry]
Type=Application
Name=Toggle Ultrawide Side Saver
Comment=Show or hide the animated side bars
Exec=$BIN toggle
Icon=video-display
Terminal=false
NoDisplay=true
# Suppress the "app is launching" busy indicator each time the shortcut fires.
StartupNotify=false
X-KDE-GlobalAccel-CommandShortcut=true
EOF

# 4. Application launcher ----------------------------------------------------
# A visible entry so the daemon can be started from Kickoff / KRunner. Launching
# it while already running is a harmless no-op (the single-instance guard exits).
say "Installing $APP_DIR/ultrawide-side-saver.desktop"
cat > "$APP_DIR/ultrawide-side-saver.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Ultrawide Side Saver
GenericName=Ultrawide Panel Saver
Comment=Animated side bars for the unused edges of an ultrawide OLED
Exec=$BIN run
Icon=video-display
Terminal=false
Categories=Utility;
Keywords=ultrawide;oled;burn-in;bars;overlay;monitor;
StartupNotify=false
EOF

# 5. Autostart ---------------------------------------------------------------
say "Installing $AUTOSTART_DIR/ultrawide-side-saver.desktop"
mkdir -p "$AUTOSTART_DIR"
cat > "$AUTOSTART_DIR/ultrawide-side-saver.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Ultrawide Side Saver
Comment=Animated side bars for the unused edges of an ultrawide OLED
Exec=$BIN run
Icon=video-display
Terminal=false
X-GNOME-Autostart-enabled=true
OnlyShowIn=KDE;
EOF

# 6. Config ------------------------------------------------------------------
say "Config: $("$BIN" init-config)"

# 7. Refresh the application menu so the launcher shows up immediately --------
update-desktop-database "$APP_DIR" 2>/dev/null || true
kbuildsycoca6 --noincremental >/dev/null 2>&1 || true

# 8. Global shortcut ---------------------------------------------------------
# Format is "active,default,friendly name".
say "Binding $SHORTCUT to toggle"
kwriteconfig6 --file kglobalshortcutsrc \
  --group services --group "$TOGGLE_DESKTOP_ID" \
  --key _launch "$SHORTCUT,none,Toggle Ultrawide Side Saver"

# In Plasma 6 kwin_wayland itself owns org.kde.kglobalaccel, and it only reads
# the services section at startup. There is no way to reload it without
# restarting the compositor, which on Wayland means restarting the session.
say "Done."
cat <<EOF

  The shortcut ($SHORTCUT) becomes active after you log out and back in.
  To use it right now without logging out, add it by hand instead:
    System Settings -> Keyboard -> Shortcuts -> Toggle Ultrawide Side Saver

  Start the daemon now:      $BIN run &
  Toggle from a terminal:    $BIN toggle
  Check output geometry:     $BIN outputs
  Edit appearance:           \${EDITOR:-nano} ${XDG_CONFIG_HOME:-\$HOME/.config}/ultrawide-side-saver/config.toml
                             $BIN reload
EOF
