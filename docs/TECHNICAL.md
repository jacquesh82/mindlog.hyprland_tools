# Technical guide

[← Overview and installation](../README.md)

## Project components

LYRA uses a local service named `hyprpanel` to communicate with Hyprland.
The service exposes 20 MCP tools. The chat adds visual choices, palettes,
contrast checks, web tools, and explicitly authorized file/command tools.

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
| `~/.local/state/hyprpanel/backups/` | Hyprland configuration backups |
| `~/.local/state/hyprpanel/file-backups/` | Previous contents saved before file-tool replacements |
| `~/.local/state/hyprpanel/lyra-history.json` | Backup records associated with chat turns |
| `~/.local/state/hyprpanel/environment.json` | Private local environment profile, created after filesystem approval |
| `~/.cache/hyprpanel/screenshots/` | Screenshots |

Configuration files are created with `600` permissions. To initialize them
without running the installer:

```bash
hyprpanel init
hyprpanel-chat init
```

### Filesystem approval

Each application session starts without assistant filesystem permission. A
native dialog asks whether to allow reading and editing configurations under
`~/.config`. Closing or declining the dialog does not grant access. Settings
lets the user grant or revoke access later; provider switching does not grant it.

Before approval, LYRA exposes text/color/SVG choices, the static palette catalog,
the contrast calculator, and public web tools. It blocks assistant file/desktop tool calls, automatic Hyprland backups,
restoration, and model-proposed local image previews. Files explicitly selected
and sent by the user are separately authorized attachments.

After approval, all three providers use the same local file tools:

| Tool | Purpose |
| --- | --- |
| `lyra_list_directory` | List up to 200 entries inside the approved configuration directory |
| `lyra_read_file` | Read a UTF-8 configuration file up to 64 KiB and return its SHA-256 |
| `lyra_write_file` | Replace a file using its current SHA-256, or create a new file with `expected_sha256: "new"` in an existing directory |

Paths are checked against the canonical approved root. Parent traversal and
symlinks escaping that directory are rejected. LYRA’s private configuration
directory is excluded, including aliases. Generated `monitors.lua` and
`inputs.lua` files stay read-only. Replacements preserve file permissions and
save the previous content under `file-backups` before an atomic write.

The generic `hypr_eval` and `hypr_dispatch` tools are not exposed or accepted by
LYRA. Shell commands use a separate, explicit per-command confirmation;
approving filesystem access alone never permits them. Specialized Hyprland tools
require filesystem approval, and IPC queries/actions require confirmation of the
displayed tool and arguments. The independent MCP service still
exposes its original tools to clients registered separately.

This is an assistant tool permission, not an OS sandbox. The application still
loads its own settings, history, assets, and provider login state. Approved MCP
backups and screenshots use LYRA’s managed data/cache directories outside
`~/.config`. Revoking access clears the model conversation for future requests;
the visible chat remains, and previously transmitted data is not recalled from
the provider. Local file/image contents may be sent to the selected provider,
as stated in the approval dialog.

### Local environment memory

After the startup filesystem consent, LYRA creates or refreshes `environment.json`
without executing commands. It reads the standard `os-release` fields, the kernel
release, a small allowlist of environment variables, and executable presence in
`PATH`. It records OS/distribution/version, architecture, username/home/shell,
graphical session, and available package managers and desktop tools. It does not
scan personal files, read credentials, or collect an environment-variable dump.

The profile uses an atomic write with mode `0600` in a `0700` state directory.
`first_seen` survives refreshes; `observed_at` records the latest observation.
Settings displays the stored JSON and offers an explicit refresh. Approved
sessions inject this profile as observed data into SYSTEM for all providers;
revocation removes it from future context. Values are never treated as instructions.
The consent dialog states that profile data can be sent to the selected provider.
Executable presence does not imply a running service or a known software version.

### Online verification

The shared SYSTEM rules instruct LYRA to verify unfamiliar, uncertain, or changing
technical information before answering or proposing actions. Custom SYSTEM roles
cannot remove these rules. Prefer official docs, upstream repositories, and release
notes; compare their version with the local system. Unknown local versions require
a confirmed diagnostic command rather than an assumption.

All subscriptions share two local tools, independent of provider-native browsing:

| Tool | Purpose |
| --- | --- |
| `lyra_web_search` | Search DuckDuckGo’s public HTML interface, returning up to six actual result titles/URLs |
| `lyra_web_fetch` | Read a public HTTPS page, returning its final URL, retrieval time, bounded text and actual navigation links |

Web requests use a dedicated HTTPS client with time/size limits. Only port 443
is accepted. Credentials in URLs, local/private IPs, and non-public DNS targets
are rejected. Resolved addresses are pinned for each request, and every redirect
is checked again. Scripts are not executed. Page contents are untrusted evidence,
not instructions. Search queries must exclude usernames, personal paths, secrets,
and local file contents. Web requests do not automatically attach the local
profile or credentials.

A search CAPTCHA, HTTP error, or empty result is reported as a failed verification.
LYRA may fetch a known official site directly and follow its actual links; it must
not invent results or say that a failed request verified anything. Source links
are included in the chat response. These are model instructions; the native tool
gateway enforces network limits and command approval independently of the model.

### Command confirmation

After filesystem approval, `lyra_run_command` accepts an exact shell script and
a reason. A native modal displays both and waits for **Run** or **Decline**.
Escape, closing, losing the target window, or an invalid/stale response denies
execution. Confirmation IDs are single-use. A refusal stops the current tool
round, defers remaining tool calls, and waits for another user message.

Approved shell commands run through `/bin/sh -c` in `~/.config`, with the user's
normal account permissions (including outside that directory). The modal explains
this boundary. There is no interactive stdin or terminal. Common credential
environment variables and shell startup hooks are removed. Output is drained
without blocking but retained only up to 32 KiB per stream; commands time out
after 60 seconds, and their process group is stopped. A timeout may leave partial
changes and never counts as success. Every later command needs a fresh approval.

Hyprland IPC queries/actions and screenshots use the same modal with their exact
tool name and JSON arguments. Managed file operations (configuration scans,
backups, local text tools) and the static event buffer do not execute external
commands. Restore confirms the restore/reload pair before running it.
Provider client startup, authentication checks, and user-invoked UI helpers are
application plumbing; provider-native shell tools remain disabled.

### SYSTEM context

Open **Settings → SYSTEM context** to define the assistant’s role and instructions.
The text is shared by all three providers and saved as `system_context` in
`chat.toml`. Saving applies it to subsequent messages in the current conversation.

An empty value uses LYRA’s default role in the selected interface language.
A custom value replaces that role. LYRA still appends the Hyprland/Wayland
scope, verification/command rules, environment context, the visual-choice protocol,
and the MCP service instructions. The editor accepts up to 16,000 characters.

The context is sent as actual system instructions: a system message for Zcode,
`--system-prompt` for Claude Code, and `baseInstructions` for Codex app-server.
It is kept separate from the user’s conversation text.

### Staying on topic

A shared SYSTEM prompt limits LYRA to Hyprland, Wayland, and their associated
tools: configuration and troubleshooting, monitors, input, appearance, bars,
launchers, locking, screenshots, screen sharing, portals, and clipboard utilities.
Related scripts and services, desktop applications, browsers, browser themes
(including unpacked Brave/Chromium themes), and questions about LYRA are in scope.

A fully unrelated request receives a short redirect with no answer to that topic
or model-requested tool calls. Mixed requests are answered only for their
relevant part. The prompt distinguishes a chocolate desktop palette from a
cake recipe and asks LYRA to stop an off-topic thread even if an earlier answer
had drifted. Custom roles remain subject to the selected scope.

**Settings → Limit actions to the Wayland desktop** is enabled by default,
including for older configurations without the new `limit_wayland_scope` key.
Disable it to answer general requests. The setting is saved in `chat.toml` and
updates SYSTEM for all providers immediately on save; no new conversation is
needed. General mode overrides earlier scope-only refusals and a custom role's
Wayland specialization. It keeps verification, color accessibility rules,
filesystem permissions, and per-command confirmation.

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

The composer also accepts **Ctrl+V**, **Shift+Insert**, or its **Paste image**
button. A one-shot Wayland clipboard read runs on a worker thread through
`wl-clipboard-rs`, with PNG/JPEG/WebP/GIF support, a 5 MiB payload cap, bounded pipe
read time, and a 32 megapixel dimension cap. No clipboard watcher or polling loop
runs while idle. Text paste falls back to Slint's native selection/undo behavior.

Pasted images are explicit attachments, so they do not require general filesystem
approval. They are staged in `0600` files under a private temporary directory,
displayed in the draft, and shared only on Send. The draft owns each temporary
image until removal/reset or transfer to the in-flight turn. The file stays alive
while the model input is prepared and is removed at the end of that turn. Normal
window closure also removes the temporary directory. Sending waits for clipboard
loading to finish without blocking typing or clearing the draft.

`lyra_offer_choices` displays 2–8 clickable options: text, colors or palettes,
local images, or self-contained SVG diagrams. Clicking sends the exact choice
back to the agent and resumes the conversation. A preference does not grant
blanket permission to modify the desktop.

`lyra_palette_catalog` provides a local selection of [Color Hunt](https://colorhunt.co/)
palettes with exact color codes and source links. It is a starting point rather
than a live catalog.

### Color contrast rules

The [color accessibility policy](COLOR_ACCESSIBILITY.md) is appended to SYSTEM
after custom roles for all three subscriptions. The original French rule lives
in `crates/hyprpanel-chat/prompts/color-accessibility.fr.md`. General mode keeps it.

`lyra_check_contrast` checks up to 32 named BG/FG roles/states per call. It linearizes
sRGB with the 0.04045 breakpoint and WCAG relative-luminance weights, then computes
`(Lmax + 0.05) / (Lmin + 0.05)` without rounding for pass/fail. It distinguishes
normal text, known large/bold text, and essential graphics. Alpha is composited
against a known opaque canvas; unknown backdrops are rejected rather than assumed.

Failing pairs get proposed opaque 8-bit repairs: adjust FG first while keeping BG;
for a 7:1 target that cannot be met on the original BG, adjust BG as well. Image and
gradient backgrounds require worst-case samples or a verified opaque surface.
Repairs must be rechecked before use. This calculator is not a full WCAG audit.

Tests cover formula reference values, exact AA boundaries, transparency, large
text thresholds, generated repairs, and the native semantic theme tokens across
default/hover/pressed/focus/disabled roles. Primary-button pressed colors and
focus indicators use validated semantic tokens.

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
hyprpanel-chat check-providers --scope
hyprpanel-chat check-providers --policy
hyprpanel-chat check-web
```

The first command checks the Coding Plan key and the official clients’ local
login state. The live test makes real subscription requests, verifies that text
fragments arrive before the final answer, checks a SYSTEM-context marker, and
validates two clickable choices. `--scope` makes real model requests to check
that restricted mode redirects a recipe while accepting Wayland and Brave-theme
requests, and that general mode answers after an earlier scope refusal.
`--policy` makes real subscription requests to verify that uncertainty produces
a web-tool request, that an exact command is proposed through LYRA’s tool,
and that inaccessible Waybar text receives a computed contrast correction.
It never executes proposed web/command actions; contrast calculation is pure.
`check-web` reads the official wiki
directly to diagnose HTTPS, DNS, and text extraction.
These diagnostics do not modify Hyprland.

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
