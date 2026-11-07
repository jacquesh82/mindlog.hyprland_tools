#!/usr/bin/env bash
# Installation de hyprpanel sur la machine locale :
#   1. build release + binaire dans ~/.local/bin
#   2. config initiale (jeton) si absente — jamais écrasée
#   3. unit systemd user (WAYLAND_DISPLAY détecté) + enable --now
# N'enregistre PAS les clients MCP : les commandes sont affichées à la fin.
set -euo pipefail
cd "$(dirname "$0")/.."

BIN_DIR="$HOME/.local/bin"
UNIT_DIR="$HOME/.config/systemd/user"

echo "== build release"
cargo build --release

echo "== binaires -> $BIN_DIR"
install -Dm755 target/release/hyprpanel "$BIN_DIR/hyprpanel"
install -Dm755 target/release/hyprpanel-chat "$BIN_DIR/hyprpanel-chat"

echo "== configuration"
"$BIN_DIR/hyprpanel" init
"$BIN_DIR/hyprpanel-chat" init

TOKEN="$("$BIN_DIR/hyprpanel" token)"
PORT="$(grep -E '^port' "$HOME/.config/hyprpanel/config.toml" | head -1 | sed 's/[^0-9]//g')"
PORT="${PORT:-8765}"

echo "== unit systemd user"
WL_DISPLAY="${WAYLAND_DISPLAY:-wayland-1}"
mkdir -p "$UNIT_DIR"
# Copie l'unit modèle et ajoute l'affichage Wayland détecté dans [Service].
sed "/^\[Service\]/a Environment=WAYLAND_DISPLAY=$WL_DISPLAY" \
    assets/hyprpanel.service > "$UNIT_DIR/hyprpanel.service"

systemctl --user daemon-reload
systemctl --user enable --now hyprpanel.service
sleep 1
systemctl --user --no-pager --lines=0 status hyprpanel.service || true

echo
echo "=================================================================="
echo " hyprpanel installé — http://127.0.0.1:$PORT/mcp"
echo "=================================================================="
echo
echo "Enregistrement Claude Code (user scope) :"
echo "  claude mcp add --scope user --transport http hyprpanel http://127.0.0.1:$PORT/mcp --header \"Authorization: Bearer $TOKEN\""
echo
echo "Enregistrement ZCode (~/.zcode/cli/config.json, clé imbriquée mcp.servers) :"
echo "  \"hyprpanel\": { \"url\": \"http://127.0.0.1:$PORT/mcp?key=$TOKEN\" }"
echo
echo "Jeton : $TOKEN"
echo
echo "Fenêtre de chat : lancez « hyprpanel-chat » (ex. dans hyprland.lua :"
echo "  hl.bind(mainMod..\"+Y\", hl.dsp.exec_cmd(\"hyprpanel-chat\")) )"
