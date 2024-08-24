# Amiel's convoluted pomodoro setup

This rust script reacts to events published to a unix socket by https://github.com/kevinschoon/pomo.

Currently, this customizes the behavior of pomo with:

* Enable/disable os-wide Do Not Disturb "Focus"

  ...to prevent being distracted by notifications.

* Set slack status with :tomato: icon and number of minutes remaining,

  ...so that my co-workers know I'm focussed and how soon they can expect a response.

* Display a dialog and use system beep when the pomodoro is complete

  ...to encourage me to actually stop working at the break by being extra annoying


## Requirements

* [pomo](https://github.com/kevinschoon/pomo)
* `Focus` and `Unfocus` shortcuts
* `slack_status` [script](https://github.com/amiel/dotfiles/blob/master/bin/slack_status)

### Setup

#### Install and configure pomo

1. Follow instructions from https://github.com/kevinschoon/pomo to install.
2. Configure to use a socket by putting the following in `~/.pomo/config.json` (don't forget to update `<your-username>` so that the path accurately reflects your home directory)

```json
{
  "publish": true,
  "publishJson": true,
  "publishSocketPath": "/Users/<your-username>/.pomo/publish.sock"
}
```

#### Set up Focus shortcuts

Use Shortcuts.app to create two shortcuts. Each uses the "Set Focus" action. I used the "Do Not Disturb" Focus, but you could use another, you'll just want to make sure that Focus silences Slack notifications.

* Focus: `Turn` `Do Not Disturb` `On` until `Turned Off`
* Unfocus: `Turn` `Do Not Disturb` `Off`

#### Set up slack_status script

1. put https://github.com/amiel/dotfiles/blob/master/bin/slack_status somewhere in your PATH
2. make a Slack API token with the `users.profile:write` permission
3. set that in your environment as `SLACK_STATUS_API_TOKEN`

#### Optional: set up tmux shortcuts

For this part to work, you must have a tmux session with the name "Pomodoro" and at least two windows.

Add the following to `~/.tmux.conf`:

```
bind-key p if-shell "pomo status|cut -f1 -d' ' |grep -q '[PR]'" { # Running or paused,
  # Pause/unpause
  send-keys -t "Pomodoro:2.1" p
} { if-shell "pomo status|cut -f1 -d' ' |grep -q B" { # Break
  # start the next pomodoro
  send-keys -t "Pomodoro:2.1" Enter
} { if-shell "pomo status|cut -f1 -d' ' |grep -q C" { # Completed
  # Quit the pomodoro screen and prompt to start a new one.
  send-keys -t "Pomodoro:2.1" q
  display "Good work, your last pomodoro set was completed. Now set up a new Pomodoro."
} { # No current pomodoro
  # Verify that the next pomodoro has not been started yet. If there is a new
  # empty pomodoro, then we can go ahead and start it. Otherwise, require
  # manual intervention.
  if-shell "test $(pomo list --json -n 1 --assend|jq '.[0].pomodoros | length') = 0" {
    send-keys -t "Pomodoro:2.1" q C-u 'pomo b $(pomo list --json -n 1 --assend | jq '\''.[0].id'\'')' Enter
  } {
    display "Your last pomodoro set was completed. Now set up a new Pomodoro."
  }
} } }
```

Then, create a pomodoro with:

```
pomo c "description"
```

Then you can use `<prefix>-t ` in tmux to:

* start the next pomodoro
* continue when on break
* pause/unpause when running
