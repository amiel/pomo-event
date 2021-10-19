use std::os::unix::net::{UnixListener, UnixStream};

use std::io::Read;

use serde::{Deserialize, Serialize};
use serde_json::Value;

const STATUS_TIME_TO_SECONDS: i64 = 1_000_000_000;
const STATUS_TIME_TO_MINUTES: i64 = 60_000_000_000;

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
    fn state(&self) -> &str {
        match self.state {
            1 => "RUNNING",
            2 => "BREAKING",
            3 => "COMPLETE",
            4 => "PAUSED",
            _ => "?",
        }
    }

    fn remaining_minutes(&self) -> i64 {
        self.remaining / STATUS_TIME_TO_MINUTES
    }

    fn remaining_seconds(&self) -> i64 {
        self.remaining / STATUS_TIME_TO_SECONDS
    }

    fn remaining_subseconds(&self) -> i64 {
        self.remaining_seconds() - (self.remaining_minutes() * 60)
    }

    fn format_remaining(&self) -> String {
        if self.remaining > 0 {
            format!(
                "{:?}:{:?}m",
                self.remaining_minutes(),
                self.remaining_subseconds()
            )
        } else {
            format!(
                "{:?}:{:?}m ago",
                -self.remaining_minutes(),
                -self.remaining_subseconds()
            )
        }
    }
}

fn handle_client(mut stream: UnixStream) -> std::io::Result<()> {
    let mut response = String::new();
    stream.read_to_string(&mut response)?;

    let v: Value = match serde_json::from_str(response.as_str()) {
        Ok(v) => v,
        Err(err) => panic!("Error decoding json {:?}", err),
    };

    let json = base64::decode(v.as_str().unwrap().to_string()).unwrap();

    let json_str: &str = std::str::from_utf8(&json).unwrap();

    let status: Status = serde_json::from_str(json_str).unwrap();

    println!("{:?}: {:?}", status.state(), status.format_remaining());

    return Ok(());
}

fn main() -> std::io::Result<()> {
    let listener = UnixListener::bind("/Users/amiel/.pomo/publish.sock")?;

    // accept connections and process them, spawning a new thread for each one
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                /* connection succeeded */
                handle_client(stream)?;
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
