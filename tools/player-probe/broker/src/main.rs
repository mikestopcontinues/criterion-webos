//! Harmless bounded pipe protocol. It has no network, account or playback code.
#![forbid(unsafe_code)]

use std::io::{BufReader, Read, Write};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::{Duration, Instant};

const MAX_LINE: usize = 48;
const MAX_SEQUENCE: u32 = 1_000_000;

enum Input {
    Ping(u32),
    Stop,
    Eof,
    Invalid,
}

fn parse_line(line: &[u8]) -> Input {
    if line == b"stop" {
        return Input::Stop;
    }
    let Some(sequence) = line.strip_prefix(b"ping ") else {
        return Input::Invalid;
    };
    if sequence.is_empty()
        || sequence.len() > 7
        || sequence[0] == b'0'
        || !sequence.iter().all(u8::is_ascii_digit)
    {
        return Input::Invalid;
    }
    let Ok(sequence) = std::str::from_utf8(sequence).unwrap().parse::<u32>() else {
        return Input::Invalid;
    };
    if sequence > MAX_SEQUENCE {
        Input::Invalid
    } else {
        Input::Ping(sequence)
    }
}

fn read_input(sender: SyncSender<Input>) {
    let mut reader = BufReader::new(std::io::stdin());
    let mut line = Vec::with_capacity(MAX_LINE);
    let mut byte = [0_u8; 1];
    loop {
        let input = match reader.read(&mut byte) {
            Ok(0) => Some(if line.is_empty() {
                Input::Eof
            } else {
                Input::Invalid
            }),
            Ok(_) if byte[0] == b'\n' => Some(parse_line(&line)),
            Ok(_) if line.len() < MAX_LINE => {
                line.push(byte[0]);
                None
            }
            _ => Some(Input::Invalid),
        };
        if let Some(input) = input {
            let terminal = !matches!(input, Input::Ping(_));
            if sender.send(input).is_err() || terminal {
                return;
            }
            line.clear();
        }
    }
}

struct Frame {
    line: String,
    acknowledged: SyncSender<bool>,
}

fn write_output(receiver: Receiver<Frame>) {
    let mut stdout = std::io::stdout().lock();
    for frame in receiver {
        let completed = stdout
            .write_all(frame.line.as_bytes())
            .and_then(|()| stdout.flush())
            .is_ok();
        let _ = frame.acknowledged.send(completed);
        if !completed {
            return;
        }
    }
}

fn emit(output: &SyncSender<Frame>, line: String) -> bool {
    let (acknowledged, receiver) = mpsc::sync_channel(1);
    output.try_send(Frame { line, acknowledged }).is_ok()
        && receiver
            .recv_timeout(Duration::from_millis(250))
            .unwrap_or(false)
}

fn run() -> i32 {
    let arguments: Vec<String> = std::env::args().skip(1).take(5).collect();
    if arguments.len() != 4 || arguments[0] != "--lease-ms" || arguments[2] != "--max-ms" {
        return 2;
    }
    let duration = |value: &str, maximum: u32| {
        if value.is_empty() || value.len() > 6 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        value
            .parse::<u32>()
            .ok()
            .filter(|number| *number >= 50 && *number <= maximum)
    };
    let Some(lease) = duration(&arguments[1], 10_000) else {
        return 2;
    };
    let Some(maximum) = duration(&arguments[3], 120_000) else {
        return 2;
    };
    if maximum < lease {
        return 2;
    }
    let lease = Duration::from_millis(u64::from(lease));
    let maximum = Instant::now() + Duration::from_millis(u64::from(maximum));
    let mut deadline = Instant::now() + lease;
    let (input, receiver) = mpsc::sync_channel(1);
    let (output, frames) = mpsc::sync_channel(1);
    std::thread::spawn(move || read_input(input));
    std::thread::spawn(move || write_output(frames));
    let pid = std::process::id();
    if !emit(&output, format!("ready {pid}\n")) {
        return 4;
    }
    let mut counter = 0_u32;
    loop {
        let wait = deadline
            .min(maximum)
            .saturating_duration_since(Instant::now());
        if wait.is_zero() {
            return 3;
        }
        match receiver.recv_timeout(wait) {
            Ok(Input::Ping(sequence)) if sequence == counter + 1 => {
                counter += 1;
                if !emit(&output, format!("pong {sequence} {counter} {pid}\n")) {
                    return 4;
                }
                deadline = Instant::now() + lease;
            }
            Ok(Input::Stop) => {
                return if emit(&output, format!("stopped {counter} {pid}\n")) {
                    0
                } else {
                    4
                };
            }
            Ok(Input::Eof) => return 0,
            Err(mpsc::RecvTimeoutError::Timeout) => return 3,
            _ => return 2,
        }
    }
}

fn main() {
    #[cfg(all(
        feature = "webos",
        target_os = "linux",
        target_arch = "arm",
        target_pointer_width = "32",
        target_endian = "little"
    ))]
    criterion_platform::auxv::warm_auxiliary_vector();
    // A timed-out reader/writer must not keep this finite process alive.
    std::process::exit(run());
}
