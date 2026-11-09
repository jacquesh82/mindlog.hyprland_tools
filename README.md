# LYRA

**Manage your Linux desktop through conversation.**

LYRA is a native assistant for **Linux desktop operations**: exploring your
environment, finding installed applications and their configurations,
troubleshooting desktop issues, and personalizing your workspace. It works with
your existing **Zcode, Claude Code, or Codex subscription**.

Work with applications, browsers, themes, configuration files, and package tools
through conversation, with local context and explicit approval before commands.
The built-in compositor integration currently targets **Hyprland on Wayland**.

The default scope covers Wayland and its desktop applications, browsers, and
themes. Turn off **Settings → Assistant → Limit actions to the Wayland desktop** to allow
broader Linux tasks and general requests. Command confirmations and filesystem
permissions and the selected execution mode still apply.

![LYRA interface with desktop suggestions, chat, and subscription selector](docs/images/lyra.png)

## What you can ask

- “Find my installed desktop tools, their versions, and configuration files.”
- “Help me troubleshoot this application's configuration.”
- “Create an unpacked browser theme using my chosen colors.”
- “Suggest three color palettes and let me choose.”
- “On Hyprland, show my monitor setup and try setting the gaps to 10.”

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

You need **Linux, Git, and Rust/Cargo**. The bundled installer uses
**systemd user services** for the local integration service.

Native compositor controls require **Hyprland on Wayland**; this integration has
been tested with **Hyprland 0.57.1 and its Lua configuration**. Install `grim` for
Wayland screenshots and `zenity` for file attachments. Image paste uses the
Wayland clipboard.

```bash
git clone https://github.com/jacquesh82/mindlog.hyprland_tools.git
cd mindlog.hyprland_tools
./scripts/install.sh
~/.local/bin/hyprpanel-chat
```

The installer builds the application, installs both binaries in `~/.local/bin`,
and starts the local Hyprland integration service.

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

Without approval, assistant file/desktop tools, command execution and automatic
configuration backups stay disabled. Explicitly attached files and commands to
copy remain available.

## Execution modes

Choose **Settings → Access → Execution mode**, then save:

| Mode | Behavior |
| --- | --- |
| **Confirm everything** (default) | Confirm each shell command and Hyprland action. Managed file edits follow filesystem consent. |
| **FULL access** | Execute requested commands, including sudo, and desktop actions without confirmation. |
| **Confirm sudo** | Run ordinary commands and desktop actions freely; confirm sudo, other privilege elevation and detected indirect commands. |
| **Copy commands only** | Analyze approved files and show copyable commands. Execute no commands, file edits, desktop actions, backups or restores. |

All modes share the same behavior across Zcode, Claude Code and Codex. Filesystem
consent and Linux account permissions still apply. Shells, interpreters, variable
expansion and recognized scripts count as indirect execution in **Confirm sudo**.
These checks operate on the proposed command text; they are not an OS sandbox.
Commands remain noninteractive; LYRA does not collect your sudo password.

## Verification and commands

When information is uncertain or depends on a recent version, LYRA is instructed
to check online, read official documentation, and link the sources actually read.
If verification fails, it must say so rather than invent an answer.

When the selected mode requires confirmation, LYRA shows the exact command and
its purpose before execution. Declining, closing, or pressing Escape runs nothing.
A “yes” in chat does not replace this dialog. In the default mode, Hyprland IPC
queries/actions also request confirmation. Managed file reads use filesystem
consent; copy-only mode blocks edits and backups.

After your first filesystem approval, LYRA records the OS/distribution, version,
kernel, architecture, username, session, and available package/desktop tools in a
private local profile. Basic OS discovery requires no commands. The profile
is refreshed each approved session and can be viewed/refreshed in Settings.
Installed tools are discovered from package metadata, desktop launchers, and your
approved configuration files. Versions and configuration paths include their source;
missing versions stay unknown. Additional package-manager queries follow the
selected execution mode; copy-only mode displays them without running them.

The local mini-RAG recalls relevant excerpts from your requests and clicked choices,
along with installed tools that match the current question. It searches locally,
without an embedding service. Relevant excerpts are included in the context sent
to your selected provider. It starts after filesystem consent and works across
conversations and providers.

## Terminal mode

The same assistant runs in a terminal, with the same subscription, tools,
execution modes, per-turn backups and memory as the window:

```bash
hyprpanel-chat cli                         # interactive session (/help)
hyprpanel-chat cli "list my SUPER binds"   # one turn, then exit
echo "why is my gap ignored?" | hyprpanel-chat cli --fs -f shot.png
```

Replies stream as they are written; tool calls appear as one summary line each
(`-v` shows full results). Choice suggestions are numbered: type a number to pick
one. Filesystem access is asked at startup (`--fs` / `--no-fs` to answer ahead).
Confirmations show the exact command and wait for `y`/`o`; when stdin is not a
terminal (pipe), every confirmation is declined. Session commands: `/new`,
`/attach`, `/detach`, `/history`, `/restore <snapshot>`, `/refresh`, `/quit`.
A one-shot turn exits with code 1 if an error was displayed.

## Settings and backups

- **⚙ Settings**: tabs for General, Access, Assistant, and Memory. Choose language, execution policy, model, SYSTEM context, and local memory preferences.
- **SYSTEM context**: set your own instructions for all providers. Leave it empty to use LYRA’s default role.
- **Local conversation memory**: disable recall and recording, or clear saved excerpts and start a new chat.
- **⟲ History**: browse saved configurations and restore a backup.
- **Tool details**: expand the link in the chat to inspect the operations performed.

Live settings let you try changes. Once filesystem access is approved, LYRA requests a configuration backup before
each message when operations are enabled. The conversation lasts for the current window session; configuration
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
