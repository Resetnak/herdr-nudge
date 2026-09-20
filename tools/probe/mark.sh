#!/bin/sh
# Writes a marker line into events.log so a capture records what was done:
#   tools/probe/mark.sh "start: manual navigation, keyboard only"
# extract.py reads these as records with event "#mark" and no env.
LOG="$(dirname "$0")/events.log"
printf '%s\t#mark\t%s\n' "$(date +%H:%M:%S)" "$*" >> "$LOG"
