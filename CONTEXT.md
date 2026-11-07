# CONTEXT — sessions du 2026-11-07/08 (hyprpanel v0.1.0 + chat v2)

## v0.1.0 — serveur MCP (commit fccf38e)
- Daemon Rust hyprpanel (rmcp 3.9 + Axum) sur 127.0.0.1:8765/mcp, auth
  Bearer/?key= (jeton dans ~/.config/hyprpanel/config.toml, 600). 20 outils.
- Hyprland 0.57.1 Lua : `hyprctl keyword` mort ; setter runtime =
  `/dispatch hl.dsp.global("conf opt val")` (ok silencieux si inconnu) ;
  getoption = état fichiers ; `/repl` retourne la valeur d'expressions Lua.
- Installé (unit systemd user, active) ; enregistré dans Claude Code (user,
  connecté) et ZCode (mcp.servers, backup .bak-hyprpanel). Monorepo .gitignore
  à jour (d415912).

## v2 — fenêtre cliente chatbot (commit 643858a)
- `crates/hyprpanel-chat` (GPL-3.0, Slint 1.18 communauté) : fenêtre sombre,
  boucle d'agent Anthropic Messages en streaming SSE (GLM-5.3 via
  api.z.ai/api/anthropic), client MCP du daemon (jeton en query — passe tel
  quel à reqwest). Clé reprise de ~/.zcode/cli/config.json dans
  ~/.config/hyprpanel/chat.toml (600, jamais affichée).
- SSE = machine à états pure (`agent::apply_event`) + 9 tests. install.sh
  installe les deux binaires ; racourci suggéré :
  hl.bind(mainMod.."+Y", hl.dsp.exec_cmd("hyprpanel-chat")).
- Vérifié : build release, tests 22/22, fenêtre ouverte (hyprctl clients),
  session MCP créée côté daemon. Écran verrouillé pendant la démo → test de
  conversation en conditions réelles à faire au déverrouillage.

## Suivis possibles
- Streaming des bulles outil (statique aujourd'hui), historique persistant,
  bouton « arrêter » pendant une génération, keybind SUPER+Y à ajouter dans
  hyprland.lua si adopté.

