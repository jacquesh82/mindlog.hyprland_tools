# CONTEXT — session du 2026-11-07 (hyprpanel v0.1.0)

- Création complète de mindlog.hyprland_tools (sous-dépôt, commit fccf38e) :
  daemon Rust hyprpanel = serveur MCP (rmcp 3.9 + Axum) sur 127.0.0.1:8765/mcp,
  auth Bearer/?key= (jeton dans ~/.config/hyprpanel/config.toml, 600).
- 20 outils : état (monitors/workspaces/clients/devices/binds), live
  (set_option_live, dispatch, notify), validation (reload, config_errors),
  backups, événements socket2, screenshot grim, eval Lua (/repl).
- Découvertes clés Hyprland 0.57.1 : config Lua hl.* ; `hyprctl keyword` mort ;
  setter runtime = `/dispatch hl.dsp.global("conf opt val")` (ok silencieux si
  inconnu) ; getoption lit les fichiers, pas les overrides ; `/repl` renvoie
  la valeur d'expressions Lua. monitors.lua/inputs.lua = hyprdmc, read-only.
- Installé : ~/.local/bin/hyprpanel + unit systemd user (active, WAYLAND_DISPLAY
  injecté). Enregistré dans Claude Code (user, connecté) et ZCode
  (~/.zcode/cli/config.json, mcp.servers, backup .bak-hyprpanel).
- Suivis possibles : v2 Slint sur hyprpanel-core ; tests d'intégration IPC
  (ignorés, absents) ; monorepo .gitignore mis à jour (commit d415912).
