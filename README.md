# LYRA

**Configure your Hyprland desktop through conversation.**

LYRA is a native chat app for exploring your setup, trying desktop settings,
and choosing a look that suits you. It works with your existing **Zcode,
Claude Code, or Codex subscription**.

By default, LYRA focuses on **Hyprland, Wayland, desktop applications, browsers,
and themes**. Turn off **Settings → Limit actions to the Wayland desktop** to
allow general requests. Command confirmations and filesystem permissions still apply.

![LYRA interface with desktop suggestions, chat, and subscription selector](docs/images/lyra.png)

## What you can ask

- “Show my monitor setup and keyboard shortcuts.”
- “Try setting the gaps to 10, then put them back.”
- “Suggest three color palettes and let me choose.”

Click LYRA’s suggestions to choose text, colors, images, or diagrams.
Use **Attach** to send images and text files along with your message.
Paste an image with **Ctrl+V**, **Shift+Insert**, or the clipboard button. It stays
in the draft until you send it; ordinary text paste still works.
Responses appear progressively as LYRA writes, with a visible thinking indicator
while it prepares its answer.
Long operations show their current phase and progress. When the amount of work
is unknown, the indicator stays indeterminate.

Interface color suggestions follow the [color accessibility policy](docs/COLOR_ACCESSIBILITY.md):
computed WCAG contrast, corrections for failing BG/FG pairs, and checks across states.

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

## Filesystem permission

At launch, LYRA asks before giving the assistant read/write access to
**`~/.config`**. Choose **Allow access** or **Decline**. Access lasts for the
session and can be revoked in Settings. File contents may be shared with the
selected provider when needed for your requests.

Without approval, assistant file/desktop tools and automatic configuration
backups stay disabled. Explicitly attached files remain available.

## Verification and commands

When information is uncertain or depends on a recent version, LYRA is instructed
to check online, read official documentation, and link the sources actually read.
If verification fails, it must say so rather than invent an answer.

**Commands need your confirmation each time.** LYRA shows the exact command and
its purpose before execution. Declining, closing, or pressing Escape runs nothing.
Filesystem permission and a “yes” in chat do not authorize a command. Hyprland
IPC queries/actions also request confirmation; managed file reads and backups
use the filesystem permission.

After your first filesystem approval, LYRA records the OS/distribution, version,
kernel, architecture, username, session, and available package/desktop tools in a
private local profile. Basic OS discovery requires no commands. The profile
is refreshed each approved session and can be viewed/refreshed in Settings.
Installed tools are discovered from package metadata, desktop launchers, and your
approved configuration files. Versions and configuration paths include their source;
missing versions stay unknown. Additional package-manager queries always ask for
command confirmation first.

The local mini-RAG recalls relevant excerpts from your requests and clicked choices,
along with installed tools that match the current question. It searches locally,
without an embedding service. Relevant excerpts are included in the context sent
to your selected provider. It starts after filesystem consent and works across
conversations and providers.

## Settings and backups

- **⚙ Settings**: choose English or French, a model, and whether the avatar animates.
- **SYSTEM context**: set your own instructions for all providers. Leave it empty to use LYRA’s default role.
- **Local conversation memory**: disable recall and recording, or clear saved excerpts and start a new chat.
- **⟲ History**: browse saved configurations and restore a backup.
- **Tool details**: expand the link in the chat to inspect the operations performed.

Live settings let you try changes. Once filesystem access is approved, LYRA requests a configuration backup before
each message. The conversation lasts for the current window session; configuration
backups and enabled local memory remain available after you close it.

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
