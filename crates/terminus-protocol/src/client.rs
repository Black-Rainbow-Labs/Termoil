//! The IPC client: connects to the running instance's socket and sends requests one at a time.
//! `terminus +command` uses it, and so does a second `terminus-gui` that hands its window to the
//! instance already running.

use std::io::{BufRead, BufReader, Write};
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::ipc::{ActionCall, Request, Response, decode_response, encode_request, socket_name};

/// A connection to a running Terminus.
pub struct Client {
    reader: BufReader<Box<dyn std::io::Read + Send>>,
    writer: Box<dyn Write + Send>,
    next_id: u64,
}

impl Client {
    /// Connects to the instance's socket, waiting up to `wait` while the pipe is busy. Fails with
    /// `NotFound` when no instance is running.
    pub fn connect(wait: Duration) -> std::io::Result<Client> {
        Client::connect_to(&socket_name(), wait)
    }

    /// Connects to a socket by name.
    pub fn connect_to(name: &str, wait: Duration) -> std::io::Result<Client> {
        let deadline = Instant::now() + wait;
        loop {
            match open(name) {
                Ok((r, w)) => return Ok(Client { reader: BufReader::new(r), writer: w, next_id: 1 }),
                Err(e) if busy(&e) && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
                Err(e) => return Err(e),
            }
        }
    }

    /// Sends one action and waits for its answer.
    pub fn call(&mut self, action: &str, args: Map<String, Value>) -> std::io::Result<Response> {
        let id = self.next_id;
        self.next_id += 1;
        let req = Request { id, call: ActionCall { action: action.to_string(), args } };
        self.writer.write_all(encode_request(&req).as_bytes())?;
        self.writer.flush()?;
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "Terminus closed the connection"));
        }
        decode_response(&line).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }
}

/// True for the error a named pipe gives while every instance is serving another client.
fn busy(e: &std::io::Error) -> bool {
    cfg!(windows) && e.raw_os_error() == Some(231)
}

/// Opens the named pipe for reading and writing.
#[cfg(windows)]
fn open(name: &str) -> std::io::Result<(Box<dyn std::io::Read + Send>, Box<dyn Write + Send>)> {
    let file = std::fs::OpenOptions::new().read(true).write(true).open(name)?;
    let reader = file.try_clone()?;
    Ok((Box::new(reader), Box::new(file)))
}

/// Connects to the Unix socket.
#[cfg(unix)]
fn open(name: &str) -> std::io::Result<(Box<dyn std::io::Read + Send>, Box<dyn Write + Send>)> {
    let stream = std::os::unix::net::UnixStream::connect(name)?;
    let reader = stream.try_clone()?;
    Ok((Box::new(reader), Box::new(stream)))
}

/// No sockets on this target.
#[cfg(not(any(windows, unix)))]
fn open(_name: &str) -> std::io::Result<(Box<dyn std::io::Read + Send>, Box<dyn Write + Send>)> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "no IPC on this platform"))
}
