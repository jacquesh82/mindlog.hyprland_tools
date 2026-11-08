# Technical guide

[← Overview and installation](../README.md)

## Project components

LYRA uses a local service named `hyprpanel` to communicate with Hyprland.
The service exposes 20 MCP tools. The chat adds visual choices and a palette catalog.

| Directory | Purpose | License |
| --- | --- | --- |
| `crates/hyprpanel-chat` | LYRA app, Slint interface, and model conversations | GPL-3.0-only |
| `crates/hyprpanel-core` | Hyprland communication, configuration inventory, and backups | MIT |
| `crates/hyprpanel-server` | Local MCP service, built as `hyprpanel` | MIT |

The service listens on `127.0.0.1:8765/mcp` by default. Requests authenticate
with `Authorization: Bearer <TOKEN>` or the `?key=<TOKEN>` query parameter.
LYRA reads the local service configuration to connect automatically.

## Configuration and data

Default locations, adjusted when the corresponding XDG variables are set:

| Location | Contents |
| --- | --- |
| `~/.config/hyprpanel/chat.toml` | Provider, models, SYSTEM context, language, animation, and GLM Coding Plan key |
| `~/.config/hyprpanel/config.toml` | Service port, token, and paths |
| `~/.local/state/hyprpanel/backups/` | Configuration backups |
| `~/.local/state/hyprpanel/lyra-history.json` | Backup records associated with chat turns |
| `~/.cache/hyprpanel/screenshots/` | Screenshots |

Configuration files are created with `600` permissions. To initialize them
without running the installer:

```bash
hyprpanel init
hyprpanel-chat init
```

### SYSTEM context

Open **Settings → SYSTEM context** to define the assistant’s role and instructions.
The text is shared by all three providers and saved as `system_context` in
`chat.toml`. Saving applies it to subsequent messages in the current conversation.

An empty value uses LYRA’s default role in the selected interface language.
A custom value replaces that role. LYRA still appends the visual-choice protocol
and the MCP service instructions. The editor accepts up to 16,000 characters.

The context is sent as actual system instructions: a system message for Zcode,
`--system-prompt` for Claude Code, and `baseInstructions` for Codex app-server.
It is kept separate from the user’s conversation text.

### Subscriptions and models

`provider` accepts `zcode`, `claude_code`, or `codex`.

| Provider | Subscription connection | Model field |
| --- | --- | --- |
| Zcode | GLM Coding Plan at `https://api.z.ai/api/anthropic` | `model` |
| Claude Code | Official client signed in to a Claude subscription | `claude_model` |
| Codex | Official app-server signed in with ChatGPT | `codex_model` |

Zcode uses LangChain Rust to stream the official Coding Plan endpoint. This
endpoint is also documented for Zcode’s subscription setup. LYRA rejects other
Zcode endpoints rather than silently switching to a metered API.
`api_key` contains the plan key; `max_tokens` limits responses in this mode.

Claude Code and Codex use their official clients’ saved subscription sessions.
LYRA excludes inherited API keys and endpoint overrides for these processes.
Claude Code checks the authentication method; Codex also verifies the app-server
account type is `chatgpt`. There is no automatic fallback to API billing.
A blank CLI model field uses the client’s default model catalog selection.

All three providers stream text before the response is complete. Claude Code
uses partial `stream-json` events; Codex uses `item/agentMessage/delta` notifications
from app-server. Their structured replies are decoded incrementally so only the
visible answer appears in the chat. Tool calls are validated before execution.

LYRA executes MCP tools, creates backup records, and manages clickable choices
for every provider. Using these features in LYRA does not require manual MCP
registration in each external client.

Official references: [Zcode subscription setup](https://docs.z.ai/devpack/tool/zcode),
[Claude Code programmatic mode](https://code.claude.com/docs/en/headless),
[Codex app-server](https://learn.chatgpt.com/docs/app-server).

## Attachments and visual choices

The file picker uses `zenity` and supports multiple attachments:

| Type | Limit per file |
| --- | --- |
| PNG, JPEG, WebP, and GIF images | 5 MiB |
| Text, configuration, and source files | 64 KiB |

Images are sent to the model as images; text file contents are included in the
message. PDF attachments and drag-and-drop are not supported.

`lyra_offer_choices` displays 2–8 clickable options: text, colors or palettes,
local images, or self-contained SVG diagrams. Clicking sends the exact choice
back to the agent and resumes the conversation. A preference does not grant
blanket permission to modify the desktop.

`lyra_palette_catalog` provides a local selection of [Color Hunt](https://colorhunt.co/)
palettes with exact color codes and source links. It is a starting point rather
than a live catalog.

## Troubleshooting

### The local service is unavailable

```bash
systemctl --user status hyprpanel
journalctl --user -u hyprpanel -n 50 --no-pager
systemctl --user restart hyprpanel
```

Run these commands from your Hyprland session. The installer configures the
service with the Wayland display detected at installation time.

`hyprpanel check` also diagnoses IPC, configuration, backups, port availability,
and `grim`. Its port check may report “occupied” when the service is already running.

### Check subscriptions, streaming, and SYSTEM context

```bash
hyprpanel-chat check-providers
hyprpanel-chat check-providers --live
```

The first command checks the Coding Plan key and the official clients’ local
login state. The live test makes real subscription requests, verifies that text
fragments arrive before the final answer, checks a SYSTEM-context marker, and
validates two clickable choices. It does not modify Hyprland.

## Use MCP from another client

This optional integration exposes the service’s tools directly to another app.

```bash
hyprpanel token
```

Replace `<TOKEN>` below with the displayed value. Adjust the port if needed.

### Claude Code

```bash
claude mcp add --scope user --transport http hyprpanel http://127.0.0.1:8765/mcp \
  --header "Authorization: Bearer <TOKEN>"
```

### Zcode

Add the server to your existing `~/.zcode/cli/config.json` configuration:

```json
{
  "mcp": {
    "servers": {
      "hyprpanel": {
        "url": "http://127.0.0.1:8765/mcp?key=<TOKEN>"
      }
    }
  }
}
```

### Exposed tools

| Area | Tools |
| --- | --- |
| State | `hypr_status`, `hypr_monitors`, `hypr_workspaces`, `hypr_clients`, `hypr_devices`, `hypr_binds` |
| Live settings | `hypr_set_option_live`, `hypr_dispatch`, `hypr_notify` |
| Configuration and backups | `hypr_config_files`, `hypr_find_setting`, `hypr_backup`, `hypr_list_backups`, `hypr_restore_backup` |
| Validation | `hypr_reload`, `hypr_config_errors` |
| Observation | `hypr_get_option`, `hypr_eval`, `hypr_recent_events`, `hypr_screenshot` |

The service provides state, live settings, backups, and validation. Persisting
changes in `hyprland.lua` uses an external client’s file-editing tools.
Generated `monitors.lua` and `inputs.lua` files are marked read-only.

## Hyprland compatibility

Tested on Hyprland 0.57.1 with Lua configuration (`hl.*`).

- Live settings use `/dispatch hl.dsp.global("conf …")`. Unknown options can be
  ignored despite an “ok” response, so their effect must be verified.
- `getoption` and `hl.get_config` return file state, which may differ from a live override.
- `/repl` reads Lua expressions from the compositor.

## Development

```bash
cargo build --workspace --locked
cargo test --workspace --locked
cargo run -p hyprpanel-chat --example preview --locked
cargo run -p hyprpanel-chat --example typing_perf --locked
```

The two examples work without a graphical session, MCP service, or model calls.
`preview` renders the actual Slint components, checks interactions, and writes
screenshots to `target/lyra-previews/`. `typing_perf` measures keyboard processing
and rendering, and verifies that avatar animation pauses while typing.
Add `--offline` after fetching dependencies to work without network access.

Markdown responses are converted to sanitized HTML and rendered with native
components. Tool details remain collapsed until expanded, and the chat follows
the response while the user stays at the bottom.

Avatar textures are prepared at 256 × 256 during the build. A shared clock limits
animation to 20 frames per second. Motion pauses during typing, modal panels,
and minimization, and can be disabled in settings (`animate_avatar`). Image
creation prompts are kept in the [asset documentation](../crates/hyprpanel-chat/assets/README.md).

Local coding-assistant directories, instruction files, contexts, and credentials
are excluded by `.gitignore`. Keep subscription credentials in their normal
client configuration locations outside this repository.
