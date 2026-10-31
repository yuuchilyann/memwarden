//! memwarden-host - Native Messaging host for the memwarden browser extension.
//!
//! Relays framed messages between the extension (stdin/stdout) and a running
//! memwarden (named pipe). It never parses extension messages, writes files,
//! logs or opens network connections. Stdout carries the protocol only, so
//! nothing else may ever be printed to it.

mod browser;
mod pipe;

use std::io;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use memwarden_host::frame::{read_frame, write_frame};
use memwarden_host::{PROTOCOL_VERSION, pipe_name};
use pipe::PipeClient;

/// How often to retry while memwarden is not running.
const RETRY: Duration = Duration::from_secs(5);

/// Pipe to memwarden while connected; shared with the stdin thread.
type Link = Arc<Mutex<Option<Arc<PipeClient>>>>;

fn main() {
    let Ok(name) = pipe_name() else { return };
    let hello = hello_message(browser::launching_browser());
    let link: Link = Arc::default();

    // Extension → memwarden. The browser closes stdin when it disconnects us.
    let to_pipe = link.clone();
    thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        while let Ok(Some(msg)) = read_frame(&mut stdin) {
            let pipe = to_pipe.lock().unwrap().clone();
            // While memwarden is not running, extension messages are dropped;
            // the extension resends its full state after every status change.
            if let Some(pipe) = pipe {
                let _ = write_frame(&mut &*pipe, &msg);
            }
        }
        std::process::exit(0);
    });

    // memwarden → extension, reconnecting whenever memwarden restarts.
    let mut announced = None;
    loop {
        let connected = PipeClient::connect(&name).map(Arc::new);
        if let Ok(pipe) = &connected
            && write_frame(&mut &**pipe, hello.as_bytes()).is_ok()
        {
            *link.lock().unwrap() = Some(pipe.clone());
            announce(&mut announced, true);
            relay_to_stdout(pipe);
            *link.lock().unwrap() = None;
        }
        announce(&mut announced, false);
        thread::sleep(RETRY);
    }
}

/// Copies memwarden's messages to the extension until the pipe closes.
fn relay_to_stdout(pipe: &PipeClient) {
    let mut reader = pipe;
    while let Ok(Some(msg)) = read_frame(&mut reader) {
        if write_frame(&mut io::stdout().lock(), &msg).is_err() {
            // The browser is gone.
            std::process::exit(0);
        }
    }
}

/// Tells the extension whether memwarden is reachable, once per change.
fn announce(last: &mut Option<bool>, connected: bool) {
    if *last == Some(connected) {
        return;
    }
    *last = Some(connected);
    let msg = format!(r#"{{"v":{PROTOCOL_VERSION},"type":"status","connected":{connected}}}"#);
    if write_frame(&mut io::stdout().lock(), msg.as_bytes()).is_err() {
        std::process::exit(0);
    }
}

/// First message on every pipe connection. Numbers only; `null` when the
/// launching browser could not be identified.
fn hello_message(browser: Option<browser::Instance>) -> String {
    let (pid, created) = match browser {
        Some((pid, created)) => (pid.to_string(), created.to_string()),
        None => ("null".into(), "null".into()),
    };
    format!(r#"{{"v":{PROTOCOL_VERSION},"type":"hello","browser_pid":{pid},"browser_created":{created}}}"#)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_message_shapes() {
        assert_eq!(
            hello_message(Some((1234, 5678))),
            r#"{"v":1,"type":"hello","browser_pid":1234,"browser_created":5678}"#
        );
        assert_eq!(hello_message(None), r#"{"v":1,"type":"hello","browser_pid":null,"browser_created":null}"#);
    }
}
