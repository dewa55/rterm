#!/usr/bin/env bash
# Installs rterm for the current user as a desktop app (no sudo):
#   ~/.local/bin/rterm                      -> symlink to target/release/rterm (rebuilds are picked up)
#   ~/.local/share/icons/hicolor/*/apps/rterm.png
#   ~/.local/share/applications/rterm.desktop
# Run ./build.sh first. ./install.sh --uninstall removes everything again.
set -euo pipefail

cd "$(dirname "$0")"
BIN_DIR="$HOME/.local/bin"
DATA_DIR="${XDG_DATA_HOME:-$HOME/.local/share}"
ICON_DIR="$DATA_DIR/icons/hicolor"
APP_FILE="$DATA_DIR/applications/rterm.desktop"
SIZES=(16 24 32 48 64 128 256 512)

refresh() {
  gtk-update-icon-cache -q -t -f "$ICON_DIR" 2>/dev/null || true
  update-desktop-database -q "$DATA_DIR/applications" 2>/dev/null || true
}

if [[ ${1-} == --uninstall ]]; then
  rm -f "$BIN_DIR/rterm" "$APP_FILE"
  for s in "${SIZES[@]}"; do rm -f "$ICON_DIR/${s}x${s}/apps/rterm.png"; done
  refresh
  echo "rterm uninstalled."
  exit 0
fi

if [[ ! -x target/release/rterm ]]; then
  echo "target/release/rterm not found; run ./build.sh first." >&2
  exit 1
fi

mkdir -p "$BIN_DIR" "$DATA_DIR/applications"
ln -sfn "$PWD/target/release/rterm" "$BIN_DIR/rterm"

for s in "${SIZES[@]}"; do
  install -Dm644 "assets/rterm-$s.png" "$ICON_DIR/${s}x${s}/apps/rterm.png"
done

# StartupWMClass matches the window's app id, so the dock groups windows under this icon.
cat > "$APP_FILE" <<EOF
[Desktop Entry]
Type=Application
Name=rterm
GenericName=Terminal
Comment=iTerm2-style terminal with tabs, split panes and shell integration
Exec=$BIN_DIR/rterm
Icon=rterm
Terminal=false
Categories=System;TerminalEmulator;
Keywords=shell;prompt;command;commandline;terminal;iterm;
StartupNotify=true
StartupWMClass=rterm
Actions=new-window;settings;

[Desktop Action new-window]
Name=New Window
Exec=$BIN_DIR/rterm

[Desktop Action settings]
Name=Edit Configuration
Exec=xdg-open $HOME/.config/rterm/config.toml
EOF

refresh
echo "rterm installed: search for \"rterm\" in Activities, or run 'rterm'."
