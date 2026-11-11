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
| `~/.local/state/hyprpanel/memory.json` | Bounded local mini-RAG records from user requests and clicked choices |
| `~/.cache/hyprpanel/screenshots/` | Screenshots |

Configuration files are created with `600` permissions. To initialize them
without running the installer:

```bash
hyprpanel init
hyprpanel-chat init
```

### Filesystem approval

Each application session starts without assistant filesystem permission. A
native dialog asks whether to allow reading and editing files under the home
folder (`~`). Credentials stay protected even after approval (`~/.ssh`,
`~/.gnupg`, `~/.password-store`, keyrings, cloud/container credentials, provider
tokens, browser profiles) along with LYRA's own settings and data. The
environment inventory still scans `~/.config` only. Closing or declining the dialog does not grant access. Settings
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
LYRA. Commands and actions follow the saved execution mode; filesystem approval
alone does not replace that policy. Specialized Hyprland tools require filesystem
approval and are unavailable in copy-only mode. The independent MCP service still
exposes its original tools to clients registered separately.

This is an assistant tool permission, not an OS sandbox. The application still
loads its own settings, history, assets, and provider login state. Approved MCP
backups and screenshots use LYRA’s managed data/cache directories outside
`~/.config`. Revoking access clears the model conversation for future requests;
the visible chat remains, and previously transmitted data is not recalled from
the provider. Local file/image contents may be sent to the selected provider,
as stated in the approval dialog.

### Local environment memory

After filesystem consent, LYRA creates or refreshes schema 2 `environment.json`.
Basic discovery reads standard `os-release` fields, kernel release, a small
allowlist of session variables, and executable presence in `PATH`. It records
OS/distribution/version, architecture, username/home/shell and graphical session.
It does not execute binaries to get their versions or collect an environment dump.

`desktop_tools` is a dynamic inventory, not a hardcoded application list. Discovery
cross-references installed package files, desktop launchers and configuration
names/launch references in the approved configuration directory.

| Mechanism | Installed metadata |
| --- | --- |
| Pacman | Local `desc`/`files` database; confirmed `pacman -Q/-Ql` fallback |
| DEB / dpkg | Installed entries in `status`, including held packages, plus architecture-aware `.list` ownership; confirmed `dpkg-query` fallback |
| RPM | Confirmed `rpm -qa` query for name, version/release and owned files |
| APK | Local installed database; confirmed `apk list --installed` fallback |
| Portage | Local package `CONTENTS` database |
| Flatpak / Snap | Confirmed application/package listing |
| Nix | Confirmed profile JSON or legacy `nix-env` JSON; derivation-derived versions are labeled |
| Homebrew | Confirmed installed formula/cask JSON, using installed versions |
| XBPS / pipx / Cargo | Confirmed installed-package JSON/text listing |
| Manual installs / AppImage | Existing executable references from configs or desktop launchers; version remains unknown |

Inventory commands use the same execution policy as chat commands, with a
60-second timeout and a 32 MiB output cap. Copy-only mode displays commands
without running them. When confirmation is required, declining stops the
remaining queries for that refresh; there is no silent shell fallback. Query
output is parsed into selected metadata, never copied raw into model context.

Each tool records the manager/package, package version and provenance, executable
paths, observed application configs, launch references (`referenced_by`), declared
package config defaults and selection evidence. A package version is not proof of
the running binary's version; a config match is not proof that it is active.
`inventory_sources` and `inventory_limits` report failures, refusals and truncation.
Unreadable, missing and manually installed versions remain unknown.

Config traversal uses the existing filesystem gateway: no access before consent,
no escape through symlinks, private LYRA/generated files protected. Credential-like
names and browser storage/cache directories are skipped. It examines at most
6,000 entries, depth 4 and 4 MiB of config text, with a 64 KiB per-file read limit.
Launch command strings are parsed as data and never executed. Native package
sources are capped at 20,000 packages and the tool inventory at 256 entries.
Selected tool metadata is capped at 1 MiB; the complete profile has a 2 MiB limit.

Profiles are written atomically with mode `0600` in a `0700` state directory.
`first_seen` survives schema migration and refreshes; `observed_at` tracks freshness.
Settings shows the full JSON and provides refresh. SYSTEM receives a compact OS
summary/tool-name index; the mini-RAG retrieves matching tool details on each send.
All three providers share this context. Revoking filesystem access removes the
profile/recalls from future context and resets the active model conversation.

### Local conversation mini-RAG

`memory.rs` implements bounded lexical retrieval (BM25, French/English tokenization)
without an embedding model, remote vector database or background indexer. Each send
retrieves up to five documents, capped at 6,000 text characters: matching historical
requests/choices and current inventory entries. The two latest user records can
supply continuity for short follow-up requests. Retrieval precedes recording the
current request, so a message cannot retrieve itself.

`memory.json` stores up to 192 excerpts of 1,800 characters with timestamps and
explicit `user_request` / `user_choice` provenance. Requests are intentions, never
evidence that an action succeeded. Assistant proposals, tool outputs, attachment
contents and images are not indexed. A conservative heuristic skips messages with
credential-like markers or URLs; it is not a general secret detector. Clicked
choices retain question/label/value/colors but discard source URLs. Writes are
atomic and private (`0600`, directory `0700`), with a 2 MiB read/write bound.

Conversation recall and recording require both filesystem consent and
`memory_enabled` (default true, including older configs). Settings can disable them
without deleting records, or explicitly clear the file and start a new conversation.
Clearing keeps the environment profile and configuration backup history. The system
profile remains available with filesystem consent even if conversation memory is off.
New Chat keeps saved memory; changing provider uses the same local store. There are
no per-keystroke scans. Relevant retrieved data is sent to the selected subscription
provider as part of its request, with explicit untrusted-data/source boundaries
before the mandatory permission, command and color policies.

### Long-operation feedback

The chat and Settings display a progress bar during inventory. Native package
loops show processed/total counts where known; config and launcher scans show
counts while their total is unknown. Each phase has its own measured progress,
not an estimated global percentage. External queries and other waits remain
indeterminate; command approval pauses the indicator. Progress events are throttled
to 80 ms. The bar uses contrast-tested semantic tokens and an accessible progress
role/value. Its small animation runs at 12.5 Hz only while needed and pauses behind
modals/minimized windows; disabling animations also freezes it.

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

### Settings tabs and execution policy

Settings uses native Slint tabs with independently scrollable pages: General
(language, avatar), Access (filesystem consent, execution policy), Assistant
(subscription/model, SYSTEM context, client status), and Memory (conversation
recall, clearing, environment profile). The footer stays visible. Unsaved values
remain available when switching tabs; Save applies all pages together. The
execution selector and Save are disabled while an operation is running.

`chat.toml` stores `operation_mode`, with `confirm_all` as the backward-compatible
default. Accepted values are `confirm_all`, `full_access`, `sudo_only`, `copy_only`.
The selected policy is appended to the shared SYSTEM instructions after custom
context, replaces older blanket confirmation text, and is enforced by local
gateways for all three providers.

| Mode | Shell | Hyprland / restore | Managed file writes |
| --- | --- | --- | --- |
| `confirm_all` | Per-command confirmation | Confirm IPC/action or restore/reload pair | User-requested writes after filesystem consent |
| `full_access` | No confirmation, including sudo | No confirmation | Same filesystem consent |
| `sudo_only` | Confirm detected elevation/indirect execution | No confirmation | Same filesystem consent |
| `copy_only` | Return `manual_only`, never spawn | Blocked, including automatic backups | Blocked at the gateway |

Copy-only still allows approved file reads, native metadata inventory, web research,
and LYRA's own preference/profile/memory storage. The command tool remains available
even without filesystem consent because it only returns a copyable code block.
The host renders that block directly; no model response is needed to make it visible.
File/MCP tools are filtered and independently checked, so stale calls cannot bypass
the mode. Execution rechecks filesystem consent and mode after a pending approval.

`sudo_only` recognizes sudo/sudoedit, doas, pkexec, su/runuser, including quoted or
escaped names and compound shell commands. Expansion, eval/source, recognized
shells/interpreters and `.sh` scripts also require confirmation because they can
delegate privilege escalation. Detection examines proposed shell text, not the
implementation of every executable; this is not an OS sandbox. All execution
retains the account's normal Linux permissions. No sudo password prompt is provided.

### Command confirmation

After filesystem approval, `lyra_run_command` accepts an exact shell script and
a reason. When required by policy, a native modal displays both and waits for
**Run** or **Decline**.
Escape, closing, losing the target window, or an invalid/stale response denies
execution. Confirmation IDs are single-use. A refusal stops the current tool
round, defers remaining tool calls, and waits for another user message.

Approved shell commands run through `/bin/sh -c` in the home folder, with the
user's normal account permissions. The modal explains
this boundary. There is no interactive stdin or terminal. Common credential
environment variables and shell startup hooks are removed. Output is drained
without blocking but retained only up to 32 KiB per stream; commands time out
after 60 seconds, and their process group is stopped. A timeout may leave partial
changes and never counts as success.

In the default **Confirm once** mode, an approval is remembered for the rest of
the session (`commands::Approvals`, in memory only): an ordinary command is keyed
by the set of programs it runs, an opaque one (shells, interpreters, variable
expansion, scripts) by its exact text. Dangerous commands
(`operations::dangerous`: elevation, deletion, disks, system packages,
shutdown/reboot, services, process killing, destructive git, download piped into
a shell, writes into system directories, unparsable commands) need a fresh
approval every time, in **Confirm once** and **Confirm sudo** alike. The modal
states the scope of each approval.

In the default mode, each Hyprland IPC action and screenshot is confirmed once
per session, using the same modal with its tool name and JSON arguments. Managed file operations (configuration scans,
backups, local text tools) and the static event buffer do not execute external
commands. Restore follows the selected policy for the restore/reload pair.
Provider client startup, authentication checks, and user-invoked UI helpers are
application plumbing; provider-native shell tools remain disabled.

### SYSTEM context

Open **Settings → Assistant → SYSTEM context** to define the assistant’s role and instructions.
The text is shared by all three providers and saved as `system_context` in
`chat.toml`. Saving applies it to subsequent messages in the current conversation.

LYRA’s role (a general desktop agent) and the identity/execution-environment
block are always sent first. A custom value is appended as the user's custom
instructions: it refines how LYRA works without replacing its identity. LYRA
still appends verification/command rules, environment context, `context.d`
texts, the visual-choice protocol, and the MCP service instructions. The editor
accepts up to 16,000 characters.

The context is sent as actual system instructions: a system message for Zcode,
`--system-prompt` for Claude Code, and `baseInstructions` for Codex app-server.
It is kept separate from the user’s conversation text.

### General agent, identity and environment

LYRA is no longer limited to Hyprland/Wayland: the shared SYSTEM prompt presents
it as a general desktop agent that plans, acts with its tools, verifies and
reports. The former `limit_wayland_scope` setting is gone; older `chat.toml`
files that still contain it load normally and the key is dropped on next save.

Every SYSTEM prompt starts with the role, then an **identity and execution
environment** block built when the conversation starts and whenever settings,
permissions, providers or the environment change (`src/identity.rs`): LYRA's version,
provider and model, interface (window or terminal), tools, filesystem access,
MCP daemon status, execution mode, and machine facts read without any command
(`/etc/os-release`, `/proc/sys/kernel/osrelease`, architecture, `XDG_CURRENT_DESKTOP`,
session type, Hyprland presence, `$SHELL`, `$LANG`). The home path appears only
after filesystem approval.

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

### Prompt caching

All three providers reuse a cached prompt prefix (tools, SYSTEM, earlier
history) instead of reprocessing it each round. The precondition is common:
the SYSTEM prompt stays byte-identical during a conversation. The per-request
memory recall and the current UTC time therefore travel in a `<turn_context>`
block at the start of the user message, marked as data that never overrides
SYSTEM rules, and the SYSTEM prompt is only rebuilt on reset or when settings,
permissions, providers or the environment change.

| Provider | Mechanism |
| --- | --- |
| Zcode | Anthropic `cache_control` breakpoints set by LangChain Rust on the SYSTEM message and on the last message (sliding prefix); the stored conversation is never modified |
| Claude Code | One content block per history message (deterministic JSON), so the history already sent stays an exact prefix that Claude Code’s own breakpoints find in its cache across processes |
| Codex | OpenAI routes its cache by thread (`prompt_cache_key`): LYRA keeps the app-server process and its ephemeral thread for the conversation and sends only new messages. A different SYSTEM prompt or model, a truncated or edited history (cancel, reset), an error or 30 minutes idle starts a fresh process |

`hyprpanel-chat check-providers --cache` makes two or three tiny real calls per
provider, as consecutive turns, and fails if no follow-up call reads tokens from
the cache. Zcode and OpenAI caching is best effort, so one miss is tolerated.
Measured on 2026-11-11: Zcode 3,136/3,199 input tokens read from cache,
Claude Code 5,923/6,233, Codex 9,344/9,578.

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

### Provider availability at startup

`providers::availability` checks the three providers concurrently, with no model
request: Zcode needs a non-empty key and the Coding Plan endpoint; Claude Code and
Codex run their official `auth status` / `login status` (15 s timeout) and report
`NotInstalled`, `NotLoggedIn` or `Timeout`. `providers::choose` (pure, tested)
keeps the selected provider when ready, otherwise falls back to the first ready one
for the session (`ChatConfig::with_session_provider`, so `chat.toml` keeps the saved
choice), otherwise reports none. The agent runs it at startup, on **Check again**,
after settings are saved, and before sending while no provider is ready. With none
ready, the window shows the provider panel and disables Send (`providers-ready`);
the terminal prints the same list and refuses the message (exit code 1 in one-shot
mode). The daemon/MCP failure message no longer exposes file paths.

### Check subscriptions, streaming, and SYSTEM context

```bash
hyprpanel-chat check-providers
hyprpanel-chat check-providers --live
hyprpanel-chat check-providers --policy
hyprpanel-chat check-providers --cache
hyprpanel-chat check-web
```

The first command checks the Coding Plan key and the official clients’ local
login state. The live test makes real subscription requests, verifies that text
fragments arrive before the final answer, checks a SYSTEM-context marker, and
validates two clickable choices. `--policy` makes real subscription requests to verify that uncertainty produces
a web-tool request, that an exact command is proposed through LYRA’s tool,
and that inaccessible Waybar text receives a computed contrast correction.
It never executes proposed web/command actions; contrast calculation is pure.
`--cache` checks that consecutive calls read the prompt prefix from each
provider’s cache (see Prompt caching).
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
