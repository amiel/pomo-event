use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;

use std::io::Read;

use serde::{Deserialize, Serialize};
use serde_json::Value;

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

fn handle_client(mut stream: UnixStream) {
    let mut response = String::new();
    stream.read_to_string(&mut response);

    let v: Value = match serde_json::from_str(response.as_str()) {
        Ok(v) => v,
        Err(err) => panic!("Error decoding json {:?}", err),
    };

    println!("GOT {:?}", v);

    let json = base64::decode(v.as_str().unwrap().to_string()).unwrap();

    let json_str: &str = std::str::from_utf8(&json).unwrap();

    let status: Status = serde_json::from_str(json_str).unwrap();

    println!(" GOT STATUS: {:?}", status);
}

fn main() -> std::io::Result<()> {
    let listener = UnixListener::bind("/Users/amiel/.pomo/pomo.sock")?;

    // accept connections and process them, spawning a new thread for each one
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                /* connection succeeded */
                thread::spawn(|| handle_client(stream));
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

// use std::io::prelude::*;
// use std::os::unix::net::UnixStream;

// fn main() -> std::io::Result<()> {
//     let mut stream = UnixStream::connect("/Users/amiel/.pomo/pomo.sock")?;
//     stream.write_all(b"OMG OMG")?;
//     let mut response = String::new();
//     stream.read_to_string(&mut response)?;
//     println!("{:?}", response);
//     Ok(())
// }
