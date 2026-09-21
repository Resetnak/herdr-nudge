# Test fixtures

Every file here was captured from Herdr 0.9.0 on the dev machine. None is
hand-written. Don't edit them; recapture instead.

```
raw/                 probe logs, exactly as tools/probe/dump.sh wrote them
events/<category>/   one plugin invocation each, generated from raw/
cli/                 read-only `herdr` queries: argv, exit code, stdout, stderr
socket/              one Herdr socket request and its reply line
sys/                 macOS tools (lsappinfo): argv, exit code, output
```

Regenerate with `tools/fixtures/extract.py`:

```sh
python3 tools/fixtures/extract.py list [LOG]    # index a probe log, with its marks
python3 tools/fixtures/extract.py write         # regenerate events/ from raw/
python3 tools/fixtures/extract.py cli NAME ARGS # capture `herdr ARGS` into cli/
python3 tools/fixtures/extract.py socket NAME METHOD PARAMS_JSON  # into socket/
python3 tools/fixtures/extract.py sys NAME PROG ARGS  # into sys/
python3 tools/fixtures/extract.py scrub         # redact the captures in place
```

## Redaction

These files are published, so the capturing machine's identity is replaced
before they are committed. `scrub` rewrites `raw/`, `cli/`, `socket/` and
`sys/` in place, and `write` then regenerates `events/` from the redacted
logs. It also runs automatically after every capture, and it is idempotent.

| Real | In the fixtures |
|---|---|
| the capturing user's account name | `dev` (so `/Users/dev/…`) |
| the machine's hostname | `dev-mac` |
| each agent session UUID | `00000000-0000-4000-8000-0000000000NN` |

No mapping from a real id to its placeholder is stored anywhere — that file
would be the leak. Numbering continues past the highest placeholder already
present, so a later capture cannot reuse an earlier session's number.

Redaction is the one edit a `raw/` log may receive. It substitutes within
lines and never adds or removes any, so the line numbers in `SELECTIONS`
stay valid.

Deliberately **not** redacted, because they are fixture content the code is
tested against: pane, tab and workspace ids, timestamps, and terminal titles
naming this project's own work (FR-4.4 composes notification text from
`title` and `terminal_title_stripped`).

**Check before committing a new capture:** `grep -ri "<your username>" tests/`
should return nothing.

To add event fixtures: mark each step while capturing (below), copy the new
part of `tools/probe/events.log` into a new file in `raw/` (never edit an
existing one), add entries to `SELECTIONS` in the script, then run `scrub`
and `write`.

## Provenance: manual vs programmatic

Manual actions (a person clicking or typing in the Herdr TUI, or a real
agent's own hooks) and programmatic ones (`herdr` CLI, socket API) have
produced different results. Every fixture records which it was, plus the
nearest log mark. Mark each step before acting:

```sh
tools/probe/mark.sh "[manual] clicked from w3:p1 to w3:p3"
tools/probe/mark.sh "[programmatic] herdr tab focus w3:t2"
```

A programmatic result is never evidence for manual behaviour, or the reverse.

## Event fixture format

```json
{
  "source": "tests/fixtures/raw/<log>:<line>",
  "captured_at": "HH:MM:SS",
  "provenance": "manual | programmatic | unknown",
  "mark": "the last mark.sh note before this event, or null",
  "why": "what this case covers (FR ids)",
  "event": "pane.agent_status_changed",
  "event_json": "<HERDR_PLUGIN_EVENT_JSON, verbatim>",
  "env": { "HERDR_...": "<verbatim>", "HERDR_PLUGIN_CONTEXT_JSON": "<verbatim>" }
}
```

A `socket/` or `sys/` capture is only safe to repeat: both run against the
live machine, and `socket` talks to the running server. `sys` stores the
bare program name, because tests match a recording by file name while the
code calls a tool by absolute path.

`event_json` and the env values are kept as strings so a test can hand the
plugin the same bytes Herdr did. Plugin root, state and config paths in `env`
belong to the probe; tests should override them.

## Catalogue

`m` = manual, `p` = programmatic, `?` = unknown.

| File | | Covers |
|---|---|---|
| `agent/blocked` | m | Claude `blocked`, user on that pane (FR-1.1) |
| `agent/blocked-user-elsewhere` | m | Claude `blocked` 24 s after the user left the pane (FR-1.4) |
| `agent/done` | m | unwatched completion (FR-1.1, FR-1.4) |
| `agent/done-user-elsewhere` | m | `done` ~20 s after the user left the pane (FR-1.4) |
| `agent/idle-watched-completion` | m | watched completion arrives as `idle` (FR-1.4) |
| `agent/idle-first-after-claim` | m | first status after Claude claims a pane |
| `agent/working` | m | never notifies |
| `agent/status-unknown-no-agent-field` | m | after Claude `/exit`: `agent` missing (FR-8.4) |
| `shell/idle-with-title-labels` | p | watched; `title`, `display_agent`, `state_labels` |
| `shell/idle-without-working` | p | `idle` with no preceding `working` stays `idle` |
| `shell/done-unwatched-failed` | p | reported `idle` arrives as `done`, `idle=failed` kept (FR-1.2, FR-1.3) |
| `shell/done-unwatched-after-handover` | p | the same, with `idle=done` |
| `shell/working-metadata-update` | p | metadata alone emits a status event (FR-1.5) |
| `shell/blocked-with-title-labels` | p | reporter sends `blocked` |
| `shell/blocked-bare` | p | optional fields missing |
| `shell/blocked-unfocused-pane` | p | pane isn't its tab's focused pane (FR-1.4) |
| `shell/status-unknown-on-release` | p | `agent_status: "unknown"` (FR-8.4) |
| `detected/shell-claim` | p | no `released` field |
| `detected/shell-release` | p | `released: true`, `final_status` |
| `detected/agent-claim-after-shell-release` | m | shell → agent handover (FR-2.4, FR-6.4) |
| `detected/agent-release-on-exit` | m | Claude `/exit`: `final_status: "idle"` (FR-6.4) |
| `detected/shell-claim-after-agent-exit` | p | agent → shell handover (FR-6.4) |
| `lifecycle/pane-closed` | m | agent pane closed with no release first (FR-6.2) |
| `lifecycle/pane-created` | ? | nested `pane` object |
| `lifecycle/tab-created` | p | `tab create --no-focus` |
| `focus/tab-focus-{tab,pane,workspace}-focused` | p | one `herdr tab focus` burst (FR-6.1) |
| `focus/tab-focus-back-pane-focused` | p | the return burst, in a different order |
| `focus/socket-pane-focus-{pane,tab,workspace}-focused` | p | socket `pane.focus`, the call FR-5.1 makes: also a burst of three, so FR-6.1's dismiss trigger fires |
| `cli/agent-get-with-session` | m | `agent_session` present (OQ-7) |
| `cli/agent-get-reported-no-session` | p | agent label, no `agent_session` (OQ-7) |
| `cli/agent-get-plain-shell` | – | `agent_not_found` on stderr, exit 1 |
| `cli/agent-manifests` | – | the catalogue (FR-2.3) |
| `cli/pane-get-focused` | – | `focused: true` for the pane the user is on (FR-1.4) |
| `cli/pane-get-unfocused` | – | `focused: false`, and it works on a plain shell pane |
| `cli/pane-list` | – | every pane; exactly one has `focused: true` |
| `socket/pane-focus-ok` | p | the focus call a click makes; reply is `{"id","result"}` |
| `socket/pane-focus-not-found` | p | `pane_not_found`, the error reply shape |
| `sys/lsappinfo-front` | – | the frontmost app's ASN |
| `sys/lsappinfo-bundleid-ghostty` | – | that ASN's bundle id |
| `sys/lsappinfo-bundleid-gone` | – | an app that has quit: `[ NULL ]`, still exit 0 |

## Findings (2026-09-18 captures)

Each finding lists the log marks that back it. Several contradict what the
documentation and other plugins assume about Herdr, so they are recorded here
with their evidence rather than only in code comments.

`FR-*`, `OQ-*` and `D-*` ids below refer to this project's requirements and
design notes, which are kept outside the repository. The findings stand on
their own without them; the ids are just cross-references.

1. **`focused_pane_id` is the event's own pane, not where the user is.**
   Manual: Claude in `w3:p4` blocked 24 s after the user clicked to `w3:p3`
   (and was `done` ~20 s after, in an earlier run). Both events said
   `focused_pane_id: "w3:p4"`. Programmatic runs agree, including a pane
   that wasn't even its tab's focused pane. Across all 196 status events in
   `raw/` it never differs from the event's pane, so FR-1.4 can't use it.
   **Use `herdr pane get <pane_id>` instead:** its `focused` field does track
   manual navigation, across panes, tabs and workspaces (polled once a second
   through a manual run, every move seen), exactly one pane is focused
   server-wide, and unlike `agent get` it works on plain shell panes. This is
   what `herdr-focus-notify` does, via `pane list`.
2. **Herdr knows where the user is anyway: an unwatched completion becomes
   `done`.** True for agents (manual) and for reported shell `idle`
   (programmatic), provided a `working` came first. Without a preceding
   `working` the `idle` stays `idle`. FR-1.2's default
   `[shell] statuses = idle` would miss the unwatched case, which is the one
   that matters. `state_labels` survive the change.
3. **Manual navigation emits no focus events.** Mouse and keyboard, across
   panes, tabs and workspaces: zero `pane/tab/workspace.focused`
   (OQ-6 regression guard). `herdr tab focus` emits all three in the same
   second, in varying order, and `workspace.focused` fires even when the
   workspace doesn't change.
4. **`--seq` persists per pane and source, across a release.** Reusing
   `--seq 1` after an earlier claim silently dropped the `working` report.
   The zsh hook's `--seq` must keep increasing across shells (FR-9.5).
5. **Metadata persists across a release.** A new claim's first event carried
   the previous claim's `title` and `state_labels`. The hook must always set
   or clear metadata.
6. **`report-metadata` alone emits `pane.agent_status_changed`**, with an
   unchanged status (FR-1.5 dedup).
7. **Claude `/exit` sends a release; closing its pane doesn't.** `/exit` gives
   `agent_detected {released: true, final_status: "idle"}` then an `unknown`
   status with no `agent` field. `pane.closed` arrived with no release first.

## Still missing

- `agent get` for a **real** agent without a Herdr integration. The
  shell-reporter case (`cli/agent-get-reported-no-session`) covers the same
  shape: label present, `agent_session` absent.
- `blocked` while the user is in another **tab** or **workspace**, manual.
  Skipped: finding 1 already rules out `focused_pane_id`.
- An `agent explain --file` replay, which would exercise agent detection
  offline against a captured screen.
