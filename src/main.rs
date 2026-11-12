use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};

use std::io::Read;
use std::process;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use time::OffsetDateTime;

// const STATUS_TIME_TO_SECONDS: i64 = 1_000_000_000;
const STATUS_TIME_TO_MINUTES: i64 = 60_000_000_000;

const STATE_UNKNOWN: u8 = 0;
const STATE_RUNNING: u8 = 1;
const STATE_BREAKING: u8 = 2;
const STATE_COMPLETE: u8 = 3;
const STATE_PAUSED: u8 = 4;

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
struct Status {
    // TODO: State is an enum:
    // RUNNING = 1
    // BREAKING = 2
    // COMPLETE = 3
    // PAUSED = 4
    state: u8,
    remaining: i64,
    count: u8,
    n_pomodoros: u8,
}

#[derive(Debug, Default, Clone)]
struct ApplicationState {
    current_status: Status,
    previous_status: Status,
    dialog_process: Arc<Mutex<Option<process::Child>>>,
}

impl Status {
    fn is_change(&self, other: &Status) -> bool {
        self.state != other.state
            || (self.remaining_minutes() != other.remaining_minutes()
                && self.state != STATE_COMPLETE)
    }

    fn state(&self) -> &str {
        match self.state {
            STATE_RUNNING => "RUNNING",
            STATE_BREAKING => "BREAKING",
            STATE_COMPLETE => "COMPLETE",
            STATE_PAUSED => "PAUSED",
            _ => "UNKNOWN",
        }
    }

    fn short_state(&self) -> &str {
        match self.state {
            STATE_RUNNING => "R",
            STATE_BREAKING => "B",
            STATE_COMPLETE => "C",
            STATE_PAUSED => "P",
            _ => "U",
        }
    }

    fn remaining_minutes(&self) -> i64 {
        // Without adding one, 59s would be 0m remaining.
        1 + self.remaining / STATUS_TIME_TO_MINUTES
    }

    // Text for the herdr sidebar's $pomo token. An empty string clears it.
    fn herdr_status(&self) -> String {
        match self.state {
            STATE_RUNNING => format!(
                "🍅 {}/{} {}m",
                self.count,
                self.n_pomodoros,
                self.remaining_minutes()
            ),
            STATE_PAUSED => format!(
                "⏸ {}/{} {}m",
                self.count,
                self.n_pomodoros,
                self.remaining_minutes()
            ),
            STATE_BREAKING => "☕ break".into(),
            // COMPLETE and UNKNOWN clear it.
            _ => "".into(),
        }
    }

    fn format_remaining(&self) -> String {
        if self.state == STATE_UNKNOWN {
            return "".into();
        }

        // Between -1 and 0 is effectively "now"
        match self.remaining_minutes() {
            0 => format!("{:?}m", self.remaining_minutes()),
            n if n < 0 => format!("{:?}m ago", -self.remaining_minutes()),
            _ => "now".into(),
        }
    }
}

fn handle_client(mut stream: UnixStream) -> std::io::Result<Status> {
    let mut response = String::new();
    stream.read_to_string(&mut response)?;

    let v: Value = match serde_json::from_str(response.as_str()) {
        Ok(v) => v,
        Err(err) => panic!("Error decoding json {:?}", err),
    };

    let json = base64::decode(v.as_str().unwrap()).unwrap();

    let json_str: &str = std::str::from_utf8(&json).unwrap();

    let status: Status = serde_json::from_str(json_str).unwrap();

    Ok(status)
}

fn update_slack(emoji: &str, message: &str) {
    process::Command::new("slack_status")
        .args([emoji, message])
        .output()
        .expect("failed to execute process");
}

fn update_herdr(text: &str) {
    // Deliberately not .expect()ing like the others: the herdr sidebar is an
    // optional display, and a missing binary or a stopped herdr server
    // shouldn't take down the pomodoro daemon.
    //
    // "Don't crash" is not "don't tell me", though. Only inspecting the spawn
    // result is how a herdr client/server protocol mismatch went unnoticed:
    // the binary ran fine, failed to reach herdr, and reported that on a
    // stderr nobody read. Both the exit status and stderr are logged now.
    let output = match process::Command::new("herdr-pomo-status")
        .args([text])
        .output()
    {
        Ok(output) => output,
        Err(err) => {
            println!("herdr-pomo-status failed to run: {:?}", err);
            return;
        }
    };

    if !output.status.success() {
        println!("herdr-pomo-status failed ({})", output.status);
    }

    // Printed even on success: herdr-pomo-status uses stderr for notes like
    // "no herdr workspace labeled ...", which is the other way the sidebar
    // status quietly stops updating.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stderr = stderr.trim_end();
    if !stderr.is_empty() {
        println!("{}", stderr);
    }
}

fn dnd(arg: &str) {
    println!("DND: {}", arg);
    process::Command::new("shortcuts")
        .args(["run", arg])
        .output()
        .expect("failed to execute process");
}

fn osascript(script: &str) -> std::process::Child {
    process::Command::new("osascript")
        .args(["-e", script])
        .spawn()
        .expect("failed to spawn osascript process")
}

fn beepbeep() {
    // beep 1 seems to get caught in a buffer and not do anything; beep 2 works, but it needs to be
    // just annoying enough to really catch my attention
    osascript("beep 8");
}

// This could be implemented on the ApplicationState?
fn alert_stop_work(app: &mut ApplicationState) {
    beepbeep();

    let mut process_ref = app.dialog_process.lock().expect("could not lock");

    let dialog_open = match process_ref.as_mut() {
        None => false, // no child process
        // if an attempt to wait for the process results in None, that means there hasn't been an
        // exit code yet, and the process is still running (therefore the dialog is open)
        Some(ref mut child) => child.try_wait().expect("could not wait").is_none(),
    };

    if dialog_open {
        println!("Didn't open another dialog because it was already open");
    } else {
        // "display dialog \"Pomodoro done {}\" buttons {} default button \"Start again\" cancel button \"Dismiss\"", // giving up after 60",
        // "{\"Dismiss\", \"Start again\"}" // TODO: Implement start again functionality
        // (would need to `send-keys -t "Pomodoro:2.1" Enter`; "Enter" or "q" when complete)

        let script = format!(
            "display dialog \"Pomodoro done {}\" buttons {} default button \"OK\"",
            app.current_status.format_remaining(),
            "{\"OK\"}",
        );

        let child = osascript(script.as_str());

        *process_ref = Some(child);
    }
}

fn pomodoro_complete(app: &ApplicationState) {
    pomodoro_off();

    let mut app = app.clone();
    thread::spawn(move || {
        alert_stop_work(&mut app);
    });
}

fn close_dialog(app: &ApplicationState) {
    let mut process_ref = app.dialog_process.lock().unwrap();
    if let Some(ref mut child) = process_ref.take() {
        // println!("Closing the dialog");
        // killing the dialog process closes the dialog
        child.kill().expect("could not stop dialog child process");
    }
}

// `entered` is false on the once-a-minute updates within the same state. Those
// still refresh Slack's countdown, but skip `shortcuts run`: every run leaves a
// macOS Live Activity behind, and enough of them pin WindowServer.
fn pomodoro_on(app: &ApplicationState, entered: bool) {
    let status = app.current_status.clone();
    let message = format!("refining - hall pass in {}m :inniebento:", status.remaining_minutes());
    println!("{}", message);
    update_slack("tomato", message.as_str());

    close_dialog(app);

    // This requires setting up a shortcut in Shortcuts.app called Focus
    if status.remaining_minutes() == 1 {
        // Turn off Do Not Disturb mode a minute early so that the pomodoro application's
        // notification works.
        // Do this in a thread so that we can schedule it for closer to the end of the pomodoro.
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(56));
            dnd("Unfocus");
        });
    } else if entered {
        dnd("Focus");
    }
}

fn pomodoro_off() {
    // This requires setting up a shortcut in Shortcuts.app called Focus
    dnd("Unfocus");
    update_slack("", "");
}

fn do_update(app: &ApplicationState) {
    // Done here rather than inside pomodoro_on/pomodoro_off because
    // pomodoro_off is shared by PAUSED, BREAKING and COMPLETE, so it can't
    // tell which state it is rendering.
    update_herdr(&app.current_status.herdr_status());

    // main() also calls this when only the remaining minutes changed.
    let entered = app.previous_status.state != app.current_status.state;

    match app.current_status.state {
        STATE_RUNNING => pomodoro_on(app, entered),
        STATE_BREAKING | STATE_COMPLETE if entered => pomodoro_complete(app),
        // Keep nagging through the break, but don't reopen a dismissed dialog.
        STATE_BREAKING => beepbeep(),
        STATE_PAUSED => pomodoro_off(),
        _ => (),
    }
}

// Ask pomo where it publishes rather than hardcoding the path.
//
// Duplicating it is what made this look broken for months: the config lived at
// ~/.pomo/config.json, pomo read ~/Library/Application Support/pomo/config.json
// instead, and pomo-event sat happily listening on a socket nothing wrote to.
// Reading the path back from pomo means the two cannot disagree, and an empty
// publishSocketPath (publish never enabled) now fails loudly at startup.
fn pomo_sock_path() -> std::io::Result<String> {
    let output = process::Command::new("pomo").arg("config").output()?;

    let config: Value = serde_json::from_slice(&output.stdout).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("could not parse `pomo config` output: {}", err),
        )
    })?;

    match config["publishSocketPath"].as_str() {
        Some(path) if !path.is_empty() => Ok(path.to_string()),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "`pomo config` reports no publishSocketPath. Set publish, \
             publishJson and publishSocketPath in the config file that \
             `pomo config` itself reports.",
        )),
    }
}

// Nothing unlinks the socket on exit, so a file left behind by the last run
// makes bind() fail with "Address already in use". Clear it first.
//
// This does not check whether something is still listening: probing with a
// connect() would feed handle_client an empty string, which panics, taking
// down the very instance we were checking for. So: don't run two at once.
fn remove_stale_socket(path: &str) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

fn main() -> std::io::Result<()> {
    let sock_path = pomo_sock_path()?;

    println!("Listening on {}", sock_path);

    remove_stale_socket(&sock_path)?;

    let listener = UnixListener::bind(&sock_path)?;

    let mut app = ApplicationState::default();
    // let mut current_status = Status::default();

    // accept connections and process them, spawning a new thread for each one
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                /* connection succeeded */
                let status = handle_client(stream)?;

                if app.current_status.is_change(&status) {
                    println!(
                        "\nUPDATE: {} {:?} ({}) {:?}",
                        OffsetDateTime::now_utc(),
                        status.state(),
                        status.short_state(),
                        status.format_remaining()
                    );
                    app.previous_status = app.current_status;
                    app.current_status = status;
                    do_update(&app);
                }
            }
            Err(err) => {
                print!("Error incoming {:?}", err);
                /* connection failed */
                break;
            }
        }
    }
    Ok(())
}
