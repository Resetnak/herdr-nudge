# herdr-nudge
A Herdr plugin that sends you a macOS notification when an agent needs you or finishes, or when a long-running command is done. Click the notification to jump to that pane. No extra installs needed, and you can configure what you get notified about.

## Long shell commands (zsh)

Herdr tells the plugin about agents by itself, but not about shell commands.
For those, the plugin writes a small zsh hook each time Herdr starts. Add this
to your `~/.zshrc`:

```zsh
if [[ -r ~/.local/state/herdr/plugins/herdr-nudge/herdr-nudge.zsh ]]; then
  source ~/.local/state/herdr/plugins/herdr-nudge/herdr-nudge.zsh
fi
```

That's the usual path. If you've set `XDG_STATE_HOME`, or Herdr keeps plugin
state somewhere else, `herdr plugin log` shows the real one after Herdr
starts: look for `zsh hook written to …`.

It does nothing outside a Herdr pane. A command that runs for 5 seconds or
more (`[shell] min_seconds`) gets a notification when it finishes, unless you
are looking at its pane. Agent CLIs and the commands in
`[shell] ignore_commands` are never reported. The notification shows the
command line as you typed it, cut at 60 characters, so anything secret you
put on a long-running command line ends up in Notification Center. Settings
reach new shells after Herdr restarts.
