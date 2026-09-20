#!/bin/sh
# Appends every plugin-visible Herdr event, plus the full HERDR_* environment,
# to events.log next to this script. Used to capture test fixtures.
LOG="$(dirname "$0")/events.log"
{ printf '%s\t%s\t%s\n' "$(date +%H:%M:%S)" "$HERDR_PLUGIN_EVENT" "$HERDR_PLUGIN_EVENT_JSON"
  env | grep -E '^HERDR_' | grep -v EVENT_JSON | sort | sed 's/^/    /'
} >> "$LOG" 2>&1
