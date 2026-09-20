# probe

Captures every event Herdr delivers to a plugin, with the full `HERDR_*`
environment (including `HERDR_PLUGIN_CONTEXT_JSON`), into `events.log`.

    herdr plugin link tools/probe
    # ... do things in Herdr ...
    cut -f1,2 tools/probe/events.log
    herdr plugin unlink probe

Subscribes to all 21 plugin-subscribable events verified on Herdr 0.9.0.
`events.log` is gitignored. Mark what you are about to do with `mark.sh`
first, so the capture records manual versus programmatic provenance, then
promote it into `tests/fixtures/` — see `tests/fixtures/README.md`.
