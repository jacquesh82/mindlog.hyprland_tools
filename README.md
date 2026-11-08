# LYRA

**Configure your Hyprland desktop through conversation.**

LYRA is a native chat app for exploring your setup, trying desktop settings,
and choosing a look that suits you. It works with your existing **Zcode,
Claude Code, or Codex subscription**.

![LYRA interface with desktop suggestions, chat, and subscription selector](docs/images/lyra.png)

## What you can ask

- “Show my monitor setup and keyboard shortcuts.”
- “Try setting the gaps to 10, then put them back.”
- “Suggest three color palettes and let me choose.”

Click LYRA’s suggestions to choose text, colors, images, or diagrams.
Use **Attach** to send images and text files along with your message.
Responses appear progressively as LYRA writes, with a visible thinking indicator
while it prepares its answer.

## Install from source

You need **Linux with Hyprland, Git, Rust/Cargo, and systemd**.
The project has been tested with **Hyprland 0.57.1 and its Lua configuration**.
Install `grim` for screenshots and `zenity` for file attachments.

```bash
git clone https://github.com/jacquesh82/mindlog.hyprland_tools.git
cd mindlog.hyprland_tools
./scripts/install.sh
~/.local/bin/hyprpanel-chat
```

The installer builds the application, installs both binaries in `~/.local/bin`,
and starts the local service that communicates with your desktop.

## Choose your subscription

Use the selector next to **Send** to choose a provider. Your selection is saved,
and you can switch between messages.

| Provider | How to connect |
| --- | --- |
| **Zcode** | LYRA imports your GLM Coding Plan key from the Zcode configuration when available. Otherwise, enter your plan key in LYRA’s settings. |
| **Claude Code** | Install the official `claude` client and sign in to your subscription with `claude auth login`. |
| **Codex** | Install the official `codex` client and sign in with your ChatGPT account using `codex login`. |

All three modes use the subscription connection and stream their text into the
chat. LYRA does not automatically fall back to a pay-as-you-go API.

## Settings and backups

- **⚙ Settings**: choose English or French, a model, and whether the avatar animates.
- **SYSTEM context**: set your own instructions for all providers. Leave it empty to use LYRA’s default role.
- **⟲ History**: browse saved configurations and restore a backup.
- **Tool details**: expand the link in the chat to inspect the operations performed.

Live settings let you try changes. LYRA requests a configuration backup before
each message. The conversation lasts for the current window session; configuration
backups remain available after you close it.

## Need help?

The **?** button explains how the chat works. To check provider connections:

```bash
~/.local/bin/hyprpanel-chat check-providers
```

See the [technical guide](docs/TECHNICAL.md) for troubleshooting, configuration,
MCP integration, and development commands.

## Licenses

The LYRA application (`hyprpanel-chat`) is licensed under **GPL-3.0-only**.
The core and server (`hyprpanel-core`, `hyprpanel-server`) are licensed under **MIT**.
