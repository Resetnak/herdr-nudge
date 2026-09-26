# herdr-nudge

A [Herdr](https://herdr.dev) plugin for macOS. It sends a notification when an
AI agent in one of your panes is waiting for you or has finished, or when a
long shell command ends. Click the notification and Herdr switches to that
pane and your terminal comes to the front.

- Agents: a notification when an agent goes `blocked` or `done` (Claude, Codex
  and the other agents Herdr detects).
- Shell commands: with the zsh hook, a notification when a command that ran
  5 seconds or longer finishes, with its exit status and how long it took.
- Nothing is posted for a pane you're looking at: the pane is focused in
  Herdr and your terminal is the app in front.
- A notification goes away by itself when its pane moves on (the agent starts
  working again, the pane is closed), and on Herdr 0.9.1 when you switch to
  the pane yourself. Closing a whole tab or workspace doesn't clear it yet.

## Requirements

- macOS, Apple Silicon or Intel. Tested on macOS 26.
- Herdr 0.9.0 or later.
- git, which `herdr plugin install` uses. On a Mac without it, the first
  `git` run offers to install Apple's Command Line Tools.
- zsh, only for shell-command notifications.

Nothing else. The notifier is bundled (terminal-notifier 3.1.0, renamed Herdr
Nudge so macOS can give it its own notification permission).

## Install

```sh
herdr plugin install justinchiasson/herdr-nudge
```

Herdr shows what the plugin will run and asks first. Agent notifications work
right away. To update, run the same command again. To remove it,
`herdr plugin uninstall herdr-nudge` (and the `.zshrc` lines, if you added
them).

Then restart Herdr once, so the plugin's startup hook writes the zsh hook:
`herdr server stop`, then `herdr`. This stops whatever is running in your
panes, so pick your moment. You can skip it if you don't want shell-command
notifications yet.

### Running its commands

The plugin's commands aren't on your `PATH`. From a shell, this finds the
installed binary (the path stays the same across updates):

```sh
nudge="$(herdr plugin list --plugin herdr-nudge --json | sed -n 's/.*"plugin_root":"\([^"]*\)".*/\1/p')/bin/herdr-nudge"
[ -x "$nudge" ] || echo "herdr-nudge isn't installed"
"$nudge" doctor
```

If you use them often, `echo "$nudge"` and put an alias to that path in your
`.zshrc`.

### First notification

From a Herdr pane, run `"$nudge" test`. The first notification makes macOS ask
whether Herdr Nudge may send notifications; allow it. Clicking the test
notification should bring you back to that pane.

macOS shows banners for about 5 seconds and then moves them to Notification
Center, where they stay clickable for an hour. To keep them on screen until
you deal with them, set Herdr Nudge to *Alerts* in System Settings →
Notifications.

## Shell commands (zsh)

Herdr tells the plugin about agents by itself, but not about shell commands.
For those, the plugin writes a small zsh hook each time Herdr starts, and your
`.zshrc` has to load it. `setup-zsh` shows the lines and the file, and adds
them only if you say yes:

```sh
"$nudge" setup-zsh
```

It writes to `$ZDOTDIR/.zshrc` if `ZDOTDIR` is set, else `~/.zshrc`, and does
nothing if the lines are already there. To add them by hand instead, this is
the block for the usual state directory (`doctor` prints the right one for
yours):

```zsh
if [[ -r ~/.local/state/herdr/plugins/herdr-nudge/herdr-nudge.zsh ]]; then
  source ~/.local/state/herdr/plugins/herdr-nudge/herdr-nudge.zsh
fi
```

The hook does nothing outside a Herdr pane. A command that runs for 5 seconds
or more (`[shell] min_seconds`) gets a notification when it finishes, unless
you are looking at its pane. Agent CLIs and the commands in
`[shell] ignore_commands` (editors, pagers, `ssh`, `tmux` and the like) are
never reported. The notification shows the command line as you typed it, cut
at 60 characters, so anything secret you put on a long-running command line
ends up in Notification Center. Settings reach new shells after Herdr
restarts.

## Commands

| Command | What it does |
|---|---|
| `doctor` | Checks the things that stop notifications: a config that doesn't load, `[agents] ignore` entries that match nothing, macOS permission and alert style, Herdr's own system toasts (which would double up), and whether `.zshrc` loads the hook. Exits 1 if something is broken. |
| `test` | Posts an agent-style notification for the pane it runs in. `test --shell` posts a shell-command one. Run it inside a Herdr pane. |
| `example-config` | Writes `config.toml` with every setting at its default and a note on each. If the file already exists it prints the example instead and leaves your file alone. |
| `setup-zsh` | Adds the zsh hook to `.zshrc`, after asking. |

The others (`--cleanup`, `--click`, and no arguments) are what Herdr and the
notifications run.

## Configuration

Optional. The file is `config.toml` in the plugin's config directory:

```sh
herdr plugin config-dir herdr-nudge
```

`example-config` writes it with every key and what it does. The ones people
change:

| Key | Default | |
|---|---|---|
| `[agents] statuses` | `["blocked", "done"]` | Agent states that notify. |
| `[agents] ignore` | `[]` | Agents to stay quiet about, by Herdr's label (`"codex"`, not `"Codex"`). |
| `[shell] min_seconds` | `5` | How long a command runs before it counts. |
| `[shell] notify_on_failure_only` | `false` | Only failed commands. |
| `[shell] ignore_commands` | editors, pagers, `ssh`, `tmux`, … | Matched on the command name. A list you set replaces the default one. |
| `[notifications] sound` | `false` | Herdr plays its own sound for these events; `true` adds ours. |
| `default_terminal` | unset | Bundle id of the app a click brings forward. Unset, it's the terminal your Herdr client runs in. It's a top-level key, so it goes above the first `[section]`. |

A key the plugin doesn't know makes it ignore the whole file and use the
defaults until it's fixed; `doctor` and `herdr plugin log` say so. The
`[shell]` settings reach the zsh hook only after Herdr restarts.

## Development

`cargo test` runs the tests. The manifest runs the committed universal binary
in `bin/`, so after changing the source, `tools/build-bin.sh` rebuilds it
(needs both Rust targets: `rustup target add x86_64-apple-darwin
aarch64-apple-darwin`), and it's committed with the change. After a Herdr
upgrade, see `tests/fixtures/README.md`.

## License

MIT. terminal-notifier is MIT too; its licence is in
`vendor/terminal-notifier-LICENSE.md`.
