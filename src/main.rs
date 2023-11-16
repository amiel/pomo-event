use std::os::unix::net::{UnixListener, UnixStream};

use std::io::Read;
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// const STATUS_TIME_TO_SECONDS: i64 = 1_000_000_000;
const STATUS_TIME_TO_MINUTES: i64 = 60_000_000_000;

const STATE_UNKNOWN: u8 = 0;
const STATE_RUNNING: u8 = 1;
const STATE_BREAKING: u8 = 2;
const STATE_COMPLETE: u8 = 3;
const STATE_PAUSED: u8 = 4;

#[derive(Serialize, Deserialize, Debug)]
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
        if self.remaining > 0 {
            format!("{:?}m", self.remaining_minutes())
        } else {
            format!("{:?}m ago", -self.remaining_minutes())
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
    Command::new("shortcuts")
        .args(&["run", arg])
        .output()
        .expect("failed to execute process");
}

fn pomodoro_on(status: &Status) {
    let message = format!("focused: {}m to break", status.remaining_minutes());
    println!("{}", message);
    update_slack("tomato", message.as_str());

    // This requires setting up a shortcut in Shortcuts.app called Focus
    if status.remaining_minutes() == 1 {
        // Turn off Do Not Disturb mode a minute early so that the pomodoro application's
        // notification works.
        dnd("Unfocus");
    } else {
        dnd("Focus");
    }
}

fn pomodoro_off() {
    // This requires setting up a shortcut in Shortcuts.app called Focus
    dnd("Unfocus");

    update_slack("", "");
}

fn do_update(status: &Status) {
    match status.state {
        STATE_RUNNING => pomodoro_on(status),
        STATE_BREAKING => pomodoro_off(),
        STATE_COMPLETE => pomodoro_off(),
        STATE_PAUSED => pomodoro_off(),
        _ => (),
    }
}

fn main() -> std::io::Result<()> {
    let listener = UnixListener::bind("/Users/amiel/.pomo/publish.sock")?;
    let mut current_status = Status {
        count: 0,
        n_pomodoros: 0,
        remaining: 0,
        state: STATE_UNKNOWN,
    };

    // accept connections and process them, spawning a new thread for each one
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                /* connection succeeded */
                let status = handle_client(stream)?;

                if current_status.is_change(&status) {
                    println!(
                        "UPDATE: {:?}: {:?}",
                        status.state(),
                        status.format_remaining()
                    );
                    current_status = status;
                    do_update(&current_status);
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
