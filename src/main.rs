use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};

use std::io::Read;
use std::process::Command;
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
    dialog_open: Arc<Mutex<()>>,
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

    // fn remaining_seconds(&self) -> i64 {
    //     self.remaining / STATUS_TIME_TO_SECONDS
    // }

    // fn remaining_subseconds(&self) -> i64 {
    //     self.remaining_seconds() - (self.remaining_minutes() * 60)
    // }

    fn format_remaining(&self) -> String {
        if self.state == STATE_UNKNOWN {
            return "".into();
        }

        if self.remaining_minutes() == 0 {
            format!("{:?}m", self.remaining_minutes())
        } else if self.remaining_minutes() < 0 {
            // Between -1 and 0 is effectively "now"
            format!("{:?}m ago", -self.remaining_minutes())
        } else {
            "now".into()
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

    let json = base64::decode(v.as_str().unwrap().to_string()).unwrap();

    let json_str: &str = std::str::from_utf8(&json).unwrap();

    let status: Status = serde_json::from_str(json_str).unwrap();

    return Ok(status);
}

fn update_slack(emoji: &str, message: &str) {
    Command::new("slack_status")
        .args(&[emoji, message])
        .output()
        .expect("failed to execute process");
}

fn dnd(arg: &str) {
    println!("DND: {}", arg);
    Command::new("shortcuts")
        .args(&["run", arg])
        .output()
        .expect("failed to execute process");
}

fn osascript(script: &str) -> std::process::Output {
    Command::new("osascript")
        .args(&["-e", script])
        .output()
        .expect("failed to execute osascript process")
}

fn beepbeep() {
    // beep 1 seems to get caught in a buffer and not do anything; beep 2 works, but it needs to be
    // just annoying enough to really catch my attention
    osascript("beep 8");
}

fn alert_stop_work(app: &ApplicationState) {
    beepbeep();

    if let Ok(_) = app.dialog_open.try_lock() {
        let script = format!(
            // "display dialog \"Pomodoro done {}\" buttons {} default button \"Start again\" cancel button \"Dismiss\"", // giving up after 60",
            "display dialog \"Pomodoro done {}\" buttons {} default button \"OK\"",
            app.current_status.format_remaining(),
            // "{\"Dismiss\", \"Start again\"}" // TODO: Implement start again functionality
            // (would need to `send-keys -t "Pomodoro:2.1" Enter`; "Enter" or "q" when complete)
            "{\"OK\"}",
        );

        let output = osascript(script.as_str());

        println!("dialog result: {:?}", output);
    } else {
        println!("Didn't open another dialog because it was already open");
    }
}

fn pomodoro_breaking(app: &ApplicationState) {
    pomodoro_off();

    let app = app.clone();
    thread::spawn(move || {
        alert_stop_work(&app);
    });
}

fn pomodoro_on(app: &ApplicationState) {
    let status = app.current_status.clone();
    let message = format!("focused: {}m to break", status.remaining_minutes());
    println!("{}", message);
    update_slack("tomato", message.as_str());

    // This requires setting up a shortcut in Shortcuts.app called Focus
    if status.remaining_minutes() == 1 {
        // Turn off Do Not Disturb mode a minute early so that the pomodoro application's
        // notification works.
        // Do this in a thread so that we can schedule it for closer to the end of the pomodoro.
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(56));
            dnd("Unfocus");
        });
    } else {
        dnd("Focus");
    }
}

fn pomodoro_off() {
    // This requires setting up a shortcut in Shortcuts.app called Focus
    dnd("Unfocus");
    update_slack("", "");
}

fn do_update(app: &ApplicationState) {
    match app.current_status.state {
        STATE_RUNNING => pomodoro_on(app),
        STATE_BREAKING => pomodoro_breaking(app),
        STATE_COMPLETE => pomodoro_breaking(app),
        STATE_PAUSED => pomodoro_off(),
        _ => (),
    }
}

fn pomo_sock_path() -> Result<String, std::env::VarError> {
    let home = std::env::var("HOME")?;

    Ok(String::from(home) + "/.pomo/publish.sock")
}

fn main() -> std::io::Result<()> {
    let listener = UnixListener::bind(pomo_sock_path().expect("Could not get $HOME"))?;

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
