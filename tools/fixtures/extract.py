#!/usr/bin/env python3
"""Promote probe captures into categorised test fixtures.

    python3 tools/fixtures/extract.py list  [LOG]   # index every record
    python3 tools/fixtures/extract.py write         # (re)write tests/fixtures/events
    python3 tools/fixtures/extract.py cli NAME ARGS # run `herdr ARGS`, save tests/fixtures/cli/NAME.json
    python3 tools/fixtures/extract.py scrub         # redact raw/ and cli/ in place, then rerun write

Parses a probe log (tools/probe/dump.sh format) and writes one JSON file per
selected record. Nothing is hand-written: `event_json` and every env value are
copied byte-for-byte from the log. Records are selected by the 1-based line
number of their header line in a raw log, so SELECTIONS stays stable as long
as raw logs are never edited. Lines written by tools/probe/mark.sh are kept
as "#mark" records and attached to the fixtures that follow them.
"""
import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
RAW_DIR = os.path.join(ROOT, "tests/fixtures/raw")
RAW = os.path.join(RAW_DIR, "events-2026-09-18.log")
OUT = os.path.join(ROOT, "tests/fixtures/events")
CLI = os.path.join(ROOT, "tests/fixtures/cli")

# These fixtures are published, so the capturing machine's identity comes out
# first. Substitutions are literal, applied everywhere, and idempotent: the
# shape of every path, title and id is preserved, only the values change.
# Session ids are mapped in order of first appearance, stably across files, so
# two fixtures referring to one session still agree. Deliberately NOT redacted:
# pane/workspace ids, timestamps, and terminal titles naming this project's own
# work — they are fixture content, and FR-4.4 composes notifications from them.
REDACTIONS = [("justinchiasson", "dev"), ("Justins-MacBook-Pro", "dev-mac")]
UUID_RE = re.compile(r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b")
PLACEHOLDER = "00000000-0000-4000-8000-%012d"

# No mapping from real id to placeholder is ever written down — that file would
# be the leak. Within one run, one session keeps one placeholder; across runs,
# numbering continues past the highest placeholder already in the fixtures, so
# a later capture can't reuse an earlier session's number.


def redact(text, uuid_map, counter):
    for old, new in REDACTIONS:
        text = text.replace(old, new)

    def swap(match):
        real = match.group(0)
        if real.startswith("00000000-0000-4000-8000-"):
            return real  # already redacted; keeps this idempotent
        if real not in uuid_map:
            counter[0] += 1
            uuid_map[real] = PLACEHOLDER % counter[0]
        return uuid_map[real]

    return UUID_RE.sub(swap, text)


def scrub():
    """Redact raw/ and cli/ in place. Run `write` afterwards to regenerate
    events/ from the redacted logs."""
    targets = [os.path.join(RAW_DIR, n) for n in sorted(os.listdir(RAW_DIR)) if n.endswith(".log")]
    targets += [os.path.join(CLI, n) for n in sorted(os.listdir(CLI)) if n.endswith(".json")]
    contents = {}
    used = [0]
    for path in targets:
        with open(path, encoding="utf-8") as f:
            contents[path] = f.read()
        for found in UUID_RE.findall(contents[path]):
            if found.startswith("00000000-0000-4000-8000-"):
                used[0] = max(used[0], int(found.rsplit("-", 1)[1]))
    uuid_map, changed = {}, 0
    for path, before in contents.items():
        after = redact(before, uuid_map, used)
        if after != before:
            with open(path, "w", encoding="utf-8") as f:
                f.write(after)
            print("redacted", os.path.relpath(path, ROOT))
            changed += 1
    print(f"{changed} file(s) changed, {len(uuid_map)} session id(s) replaced")

# Provenance: "manual" = a person or a real agent did it (typing, clicking, an
# agent's own hooks); "programmatic" = driven by the herdr CLI or socket;
# "unknown" = the log doesn't say. Manual and programmatic have been seen to
# behave differently, so never let one stand in for the other.
#
# {raw log: [(category, name, header line, provenance, why)]}
SELECTIONS = {
    "events-2026-09-18.log": [
        ("agent", "blocked", 118, "manual", "Claude asks for permission (FR-1.1)"),
        ("agent", "done", 430, "manual", "Claude finished while the user was away: the unwatched completion (FR-1.1, FR-1.4)"),
        ("agent", "idle-watched-completion", 92, "manual", "Claude finished while watched: Herdr reports idle, not done (FR-1.4)"),
        ("agent", "working", 105, "manual", "Claude working: never notifies"),
        ("agent", "idle-first-after-claim", 832, "manual", "First status after Claude claimed a pane a shell reporter had released"),
        ("shell", "idle-with-title-labels", 1, "programmatic", "Shell report with title, display_agent and state_labels, watched (FR-1.2, FR-4.4)"),
        ("shell", "blocked-with-title-labels", 14, "programmatic", "Shell reporter sending blocked, all optional fields present"),
        ("shell", "blocked-bare", 27, "programmatic", "Shell report with no title, display_agent or state_labels (fields missing, not null)"),
        ("shell", "idle-without-working", 327, "programmatic", "Shell idle with no preceding working (design 5.7)"),
        ("shell", "status-unknown-on-release", 40, "programmatic", "agent_status \"unknown\" sent when the reporter releases (FR-8.4)"),
        ("detected", "shell-claim", 326, "programmatic", "Shell reporter claims a pane: no released/final_status fields"),
        ("detected", "shell-release", 41, "programmatic", "Shell reporter releases: released=true, final_status (FR-2.4)"),
        ("detected", "agent-claim-after-shell-release", 819, "manual", "Claude detected in a pane a shell reporter used earlier (FR-2.4, FR-6.4)"),
        ("lifecycle", "pane-closed", 807, "manual", "Agent pane closed; no agent_detected release precedes it (FR-6.2)"),
        ("lifecycle", "pane-created", 1222, "unknown", "New plain pane: nested pane object, not flat fields"),
    ],
    "events-2026-09-18-gaps.log": [
        ("agent", "blocked-user-elsewhere", 939, "manual", "Claude blocked 24 s after the user moved to another pane; focused_pane_id is still the event's pane (FR-1.4)"),
        ("agent", "done-user-elsewhere", 833, "manual", "Claude done ~20 s after the user moved away; focused_pane_id is still the event's pane (FR-1.4)"),
        ("agent", "status-unknown-no-agent-field", 1071, "manual", "Status event after Claude /exit: agent field missing entirely (FR-8.4, invariant 5)"),
        ("detected", "agent-release-on-exit", 1058, "manual", "Claude /exit: released=true, final_status=idle (FR-2.4, FR-6.4)"),
        ("detected", "shell-claim-after-agent-exit", 1232, "programmatic", "Shell reporter claims the pane Claude just left: agent -> shell handover (FR-6.4)"),
        ("shell", "done-unwatched-failed", 94, "programmatic", "Reported idle on an unwatched pane arrives as done; state_labels idle=failed kept (FR-1.2, FR-1.3)"),
        ("shell", "done-unwatched-after-handover", 1258, "programmatic", "Reported idle arrives as done after working, in the handover pane (FR-1.2)"),
        ("shell", "working-metadata-update", 81, "programmatic", "report-metadata alone emits a status event with an unchanged status (FR-1.5)"),
        ("shell", "blocked-unfocused-pane", 203, "programmatic", "Blocked on a pane that is not its tab's focused pane; focused_pane_id is the event's pane (FR-1.4)"),
        ("lifecycle", "tab-created", 2, "programmatic", "herdr tab create --no-focus"),
        ("focus", "tab-focus-tab-focused", 1552, "programmatic", "herdr tab focus w3:t2: burst of three in one second (FR-6.1)"),
        ("focus", "tab-focus-pane-focused", 1553, "programmatic", "Same burst: pane.focused for the tab's focused pane"),
        ("focus", "tab-focus-workspace-focused", 1554, "programmatic", "Same burst: workspace.focused fires even within one workspace; no pane_id"),
        ("focus", "tab-focus-back-pane-focused", 1606, "programmatic", "herdr tab focus w3:t1: second burst, different event order"),
    ],
    "events-2026-09-20-socket-focus.log": [
        ("focus", "socket-pane-focus-pane-focused", 4, "programmatic", "socket pane.focus (the call FR-5.1 makes) emits pane.focused, so FR-6.1's dismiss trigger fires"),
        ("focus", "socket-pane-focus-tab-focused", 2, "programmatic", "Same burst: tab.focused, even though the tab did not change"),
        ("focus", "socket-pane-focus-workspace-focused", 3, "programmatic", "Same burst: workspace.focused, even though the workspace did not change"),
    ],
}


def parse(path):
    """Return records in log order: header fields plus the env block that
    dump.sh wrote for it. Events fired in the same instant write their header
    lines first and their env blocks afterwards in any order, so each env block
    is matched to the oldest pending header with the same HERDR_PLUGIN_EVENT."""
    records, pending, block = [], [], None

    def flush():
        if block is None:
            return
        name = block["env"].get("HERDR_PLUGIN_EVENT")
        for i, rec in enumerate(pending):
            if rec["event"] == name:
                rec["env"] = block["env"]
                pending.pop(i)
                return
        raise SystemExit(f"{path}:{block['line']}: env block for {name!r} has no header")

    with open(path, encoding="utf-8") as f:
        for lineno, raw in enumerate(f, 1):
            line = raw.rstrip("\n")
            if line.startswith("    "):
                key, sep, value = line[4:].partition("=")
                if not sep:
                    raise SystemExit(f"{path}:{lineno}: malformed env line")
                if block is None or key in block["env"]:
                    flush()
                    block = {"line": lineno, "env": {}}
                block["env"][key] = value
                continue
            flush()
            block = None
            time, event, event_json = line.split("\t", 2)
            rec = {"line": lineno, "time": time, "event": event, "event_json": event_json}
            records.append(rec)
            if event != "#mark":  # tools/probe/mark.sh: no env block follows
                pending.append(rec)
    flush()
    if pending:
        raise SystemExit(f"{path}: headers without env: {[r['line'] for r in pending]}")
    return records


def summary(rec):
    if rec["event"] == "#mark":
        return f"{rec['line']:>5} {rec['time']} ---- {rec['event_json']}"
    data = json.loads(rec["event_json"])["data"]
    ctx = json.loads(rec["env"].get("HERDR_PLUGIN_CONTEXT_JSON", "{}"))
    extra = {k: v for k, v in data.items() if k not in ("type", "workspace_id")}
    return (f"{rec['line']:>5} {rec['time']} {rec['event']:<26} {json.dumps(extra)[:110]}"
            f"  | focused={ctx.get('focused_pane_id')}")


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else "list"
    if cmd == "list":
        for rec in parse(sys.argv[2] if len(sys.argv) > 2 else RAW):
            print(summary(rec))
    elif cmd == "write":
        write()
    elif cmd == "cli" and len(sys.argv) > 3:
        cli(sys.argv[2], sys.argv[3:])
    elif cmd == "scrub":
        scrub()
    else:
        raise SystemExit(__doc__)


def cli(name, argv):
    """Run a read-only herdr query and store its exit code and stdout verbatim."""
    import datetime
    import subprocess

    herdr = os.environ.get("HERDR_BIN_PATH") or os.path.expanduser("~/.local/bin/herdr")
    version = subprocess.run([herdr, "--version"], capture_output=True, text=True).stdout.strip()
    proc = subprocess.run([herdr] + argv, capture_output=True, text=True)
    fixture = {
        "argv": ["herdr"] + argv,
        "captured_at": datetime.datetime.now().isoformat(timespec="seconds"),
        "herdr_version": version,
        "exit_code": proc.returncode,
        "stdout": proc.stdout,
        "stderr": proc.stderr,
    }
    path = os.path.join(ROOT, "tests/fixtures/cli", name + ".json")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        json.dump(fixture, f, indent=2, ensure_ascii=False)
        f.write("\n")
    scrub()  # a live capture carries the machine's identity; never leave it unredacted
    print(os.path.relpath(path, ROOT), "exit", proc.returncode)


def write():
    import shutil

    shutil.rmtree(OUT, ignore_errors=True)  # events/ is fully generated
    index = []
    for log, selections in SELECTIONS.items():
        raw = os.path.join(RAW_DIR, log)
        rel_raw = os.path.relpath(raw, ROOT)
        records = parse(raw)
        by_line = {r["line"]: r for r in records}
        for category, name, line, provenance, why in selections:
            rec = by_line.get(line)
            if rec is None or rec["event"] == "#mark":
                raise SystemExit(f"no event header at {rel_raw}:{line}")
            marks = [r for r in records if r["event"] == "#mark" and r["line"] < line]
            fixture = {
                "source": f"{rel_raw}:{line}",
                "captured_at": rec["time"],
                "provenance": provenance,
                "mark": marks[-1]["event_json"] if marks else None,
                "why": why,
                "event": rec["event"],
                "event_json": rec["event_json"],
                "env": rec["env"],
            }
            path = os.path.join(OUT, category, name + ".json")
            if os.path.exists(path):
                raise SystemExit(f"duplicate fixture name {category}/{name}")
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "w", encoding="utf-8") as f:
                json.dump(fixture, f, indent=2, ensure_ascii=False)
                f.write("\n")
            index.append(os.path.relpath(path, ROOT))
    print("\n".join(index))


if __name__ == "__main__":
    main()
