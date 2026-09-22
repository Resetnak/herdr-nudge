#!/usr/bin/env python3
"""Feed a captured event through the real binary, as Herdr would.

Herdr talks to a plugin entirely through environment variables, so an event
hook can be driven by hand. This sets up the same variables from a committed
fixture and runs our binary, which posts a real notification.

Point it at a live pane, or classification and the visibility check have
nothing to work with: the pane ids in the fixtures belong to panes that are
long gone, and `herdr pane get` will fail on them.

    tools/replay-event.py agent/blocked --pane w3:p1
    tools/replay-event.py shell/done-unwatched-failed --pane w3:p2
    herdr pane list      # to find a pane id

The state directory defaults to the one a click will look in, so the
notification this posts is actually clickable.
"""

import argparse
import json
import os
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
DEFAULT_STATE = pathlib.Path.home() / ".local/state/herdr/plugins/herdr-nudge"
DEFAULT_CONFIG = pathlib.Path.home() / ".config/herdr/plugins/config/herdr-nudge"


def load(name):
    path = ROOT / "tests/fixtures/events" / f"{name}.json"
    if not path.exists():
        sys.exit(f"no fixture {name}. Look under tests/fixtures/events/.")
    return json.loads(path.read_text())


def retarget_context(context_json, pane_id):
    """Keep the workspace label, fix the ids so they match the live pane."""
    context = json.loads(context_json)
    context["workspace_id"] = pane_id.split(":")[0]
    # The captured tab belongs to another workspace, and nothing we do needs
    # it. Better absent than wrong.
    context.pop("tab_id", None)
    context.pop("focused_pane_id", None)
    return json.dumps(context)


def seed_agents(state_dir, herdr):
    """Write agents-cache.json from the live server.

    Nothing writes this file yet, so without it the agent catalogue is empty
    and a real agent classifies as a shell command.
    """
    done = subprocess.run(
        [str(herdr), "server", "agent-manifests", "--json"],
        capture_output=True,
        text=True,
    )
    if done.returncode != 0:
        sys.exit(f"could not fetch agent manifests:\n{done.stderr.strip()}")
    manifests = json.loads(done.stdout)["result"]["manifests"]
    agents = sorted({m["agent"] for m in manifests})
    cache = {"version": 1, "fetched_at_ms": 0, "agents": agents}
    path = state_dir / "agents-cache.json"
    path.write_text(json.dumps(cache, indent=2) + "\n")
    print(f"seeded {path.name} with {len(agents)} agents: {' '.join(agents)}\n")


def retarget(event_json, pane_id):
    """Rewrite the captured pane and workspace so a live pane answers."""
    payload = json.loads(event_json)
    data = payload.get("data")
    if not isinstance(data, dict):
        sys.exit("this fixture has no data object to retarget")
    if "pane_id" not in data:
        sys.exit("this fixture carries no pane_id")
    data["pane_id"] = pane_id
    if "workspace_id" in data:
        # Pane ids look like w3:p1, and the workspace is the part before the
        # colon. Checked against every fixture: they always agree.
        data["workspace_id"] = pane_id.split(":")[0]
    return json.dumps(payload)


def main():
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("fixture", help="e.g. agent/blocked")
    parser.add_argument("--pane", help="a live pane id, from `herdr pane list`")
    parser.add_argument("--state-dir", type=pathlib.Path, default=DEFAULT_STATE)
    parser.add_argument("--config-dir", type=pathlib.Path, default=DEFAULT_CONFIG)
    parser.add_argument(
        "--binary",
        type=pathlib.Path,
        default=ROOT / "target/debug/herdr-nudge",
        help="default: target/debug/herdr-nudge",
    )
    parser.add_argument(
        "--herdr",
        type=pathlib.Path,
        default=pathlib.Path.home() / ".local/bin/herdr",
    )
    parser.add_argument(
        "--socket",
        type=pathlib.Path,
        default=None,
        help="default: $HERDR_SOCKET_PATH, else ~/.config/herdr/herdr.sock",
    )
    parser.add_argument(
        "--seed-agents",
        action="store_true",
        help="write agents-cache.json from the live server first",
    )
    args = parser.parse_args()

    if not args.binary.exists():
        sys.exit(f"{args.binary} not found. Run: cargo build")
    notifier = ROOT / "vendor/HerdrNudge.app/Contents/MacOS/terminal-notifier"
    if not notifier.exists():
        sys.exit(f"{notifier} not found. Run: tools/bundle/build.sh")

    fixture = load(args.fixture)
    event_json = fixture["event_json"]
    if args.pane:
        event_json = retarget(event_json, args.pane)

    # Only the context comes from the capture, and only because it is event
    # data. Every path in a fixture's environment belongs to the probe plugin
    # and has been through redaction, so `/Users/dev/...` in one is nobody's
    # real home.
    context_json = fixture["env"].get("HERDR_PLUGIN_CONTEXT_JSON")
    if context_json and args.pane:
        context_json = retarget_context(context_json, args.pane)

    socket = args.socket or pathlib.Path(
        os.environ.get("HERDR_SOCKET_PATH")
        or pathlib.Path.home() / ".config/herdr/herdr.sock"
    )
    if not socket.exists():
        print(f"warning: no socket at {socket}. Is Herdr running?\n")

    env = {k: v for k, v in os.environ.items() if not k.startswith("HERDR_")}
    env.update(
        {
            "HERDR_ENV": "1",
            "HERDR_PLUGIN_EVENT": fixture["event"],
            "HERDR_PLUGIN_EVENT_JSON": event_json,
            "HERDR_PLUGIN_ID": "herdr-nudge",
            "HERDR_PLUGIN_ROOT": str(ROOT),
            "HERDR_PLUGIN_STATE_DIR": str(args.state_dir),
            "HERDR_PLUGIN_CONFIG_DIR": str(args.config_dir),
            "HERDR_BIN_PATH": str(args.herdr),
            "HERDR_SOCKET_PATH": str(socket),
        }
    )
    if context_json:
        env["HERDR_PLUGIN_CONTEXT_JSON"] = context_json
    if args.pane:
        env["HERDR_PANE_ID"] = args.pane
        env["HERDR_WORKSPACE_ID"] = args.pane.split(":")[0]

    args.state_dir.mkdir(parents=True, exist_ok=True)

    print(f"fixture   {args.fixture}  ({fixture['why']})")
    print(f"event     {fixture['event']}")
    print(f"pane      {args.pane or '(as captured, likely gone)'}")
    print(f"state     {args.state_dir}")
    print(f"socket    {socket}")
    print(f"binary    {args.binary}\n")

    if args.seed_agents:
        seed_agents(args.state_dir, args.herdr)
    elif not (args.state_dir / "agents-cache.json").exists():
        print(
            "warning: no agents-cache.json, so the agent catalogue is empty and\n"
            "         a real agent will classify as a shell command. Re-run with\n"
            "         --seed-agents to fetch it.\n"
        )

    done = subprocess.run([str(args.binary)], env=env, capture_output=True, text=True)
    sys.stdout.write(done.stdout)
    sys.stderr.write(done.stderr)
    print(f"\nexit {done.returncode}")

    jobs = sorted((args.state_dir / "jobs").glob("*.json"))
    if jobs:
        print("\njobs waiting for a click:")
        for path in jobs:
            print(f"  {path.name}")
        print(f"\n  {args.binary} --click {jobs[-1].stem}")


if __name__ == "__main__":
    main()
