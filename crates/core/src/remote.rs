// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! The agent interface over a byte stream, for front ends without D-Bus: a named pipe on
//! Windows. Requests and replies are JSON, one per line; [`Remote`] is the client of
//! `kudari mcp` that sends them, and waits while the user is asked about the agent.

use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};
use std::thread::sleep;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::agent::{Error, Request};
use crate::mcp::Instance;

/// The longest line either side reads; requests are far shorter, and a reply holds at most
/// the lines an agent may read.
pub const MAX_LINE: u64 = 16 * 1024 * 1024;

/// A request as one line.
#[must_use]
pub fn encode_request(request: &Request) -> String {
    let value = match request {
        Request::Terminals => json!({ "method": "terminals" }),
        Request::Read { terminal, lines } => json!({ "method": "read", "terminal": terminal, "lines": lines }),
        Request::Type {
            terminal,
            text,
            execute,
        } => {
            json!({ "method": "type", "terminal": terminal, "text": text, "execute": execute })
        }
        Request::Key { terminal, key } => json!({ "method": "key", "terminal": terminal, "key": key }),
        Request::Open { title } => json!({ "method": "open", "title": title }),
        Request::Close { session } => json!({ "method": "close", "session": session }),
    };
    value.to_string()
}

/// A request read from one line.
///
/// # Errors
/// A sentence for the agent when the line is no request.
pub fn decode_request(line: &str) -> Result<Request, String> {
    let value: Value = serde_json::from_str(line).map_err(|_| "The request is no JSON.".to_string())?;
    let number = |name: &str| -> Result<i64, String> {
        value
            .get(name)
            .and_then(Value::as_i64)
            .ok_or_else(|| format!("The request needs the number {name}."))
    };
    let int = |name: &str| number(name).and_then(|n| i32::try_from(n).map_err(|_| format!("{name} is out of range.")));
    let text = |name: &str| -> Result<String, String> {
        value
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| format!("The request needs the text {name}."))
    };
    match value.get("method").and_then(Value::as_str) {
        Some("terminals") => Ok(Request::Terminals),
        Some("read") => Ok(Request::Read {
            terminal: int("terminal")?,
            lines: u32::try_from(number("lines")?).map_err(|_| "lines is out of range.".to_string())?,
        }),
        Some("type") => Ok(Request::Type {
            terminal: int("terminal")?,
            text: text("text")?,
            execute: value.get("execute").and_then(Value::as_bool).unwrap_or(false),
        }),
        Some("key") => Ok(Request::Key {
            terminal: int("terminal")?,
            key: text("key")?,
        }),
        Some("open") => Ok(Request::Open { title: text("title")? }),
        Some("close") => Ok(Request::Close {
            session: int("session")?,
        }),
        _ => Err("The request names no known method.".to_string()),
    }
}

/// A reply as one line.
#[must_use]
pub fn encode_reply(reply: &Result<String, Error>) -> String {
    let value = match reply {
        Ok(text) => json!({ "ok": text }),
        Err(error) => {
            let kind = match error {
                Error::Denied(_) => "denied",
                Error::Waiting => "waiting",
                Error::Invalid(_) => "invalid",
                Error::Failed(_) => "failed",
            };
            json!({ "error": kind, "message": error.message() })
        }
    };
    value.to_string()
}

/// A reply read from one line.
///
/// # Errors
/// A sentence for the agent when the line is no reply.
pub fn decode_reply(line: &str) -> Result<Result<String, Error>, String> {
    let value: Value = serde_json::from_str(line).map_err(|_| "Kudari answered with no JSON.".to_string())?;
    if let Some(text) = value.get("ok").and_then(Value::as_str) {
        return Ok(Ok(text.to_string()));
    }
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    match value.get("error").and_then(Value::as_str) {
        Some("denied") => Ok(Err(Error::Denied(message))),
        Some("waiting") => Ok(Err(Error::Waiting)),
        Some("invalid") => Ok(Err(Error::Invalid(message))),
        Some("failed") => Ok(Err(Error::Failed(message))),
        _ => Err("Kudari answered with no reply.".to_string()),
    }
}

/// Reads one line of at most [`MAX_LINE`] bytes; `None` at the end of the stream.
///
/// # Errors
/// What the stream reports, and a line that is too long or no UTF-8.
pub fn read_line(reader: &mut impl BufRead) -> std::io::Result<Option<String>> {
    let mut line = Vec::new();
    let read = reader.take(MAX_LINE).read_until(b'\n', &mut line)?;
    if read == 0 {
        return Ok(None);
    }
    if line.last() != Some(&b'\n') && read as u64 == MAX_LINE {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "line too long"));
    }
    String::from_utf8(line)
        .map(|line| Some(line.trim_end().to_string()))
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "no UTF-8"))
}

/// The running Kudari at the other end of a stream, as `kudari mcp` reaches it. The
/// stream is one agent for Kudari, which asks the user about it once.
pub struct Remote<S> {
    stream: Mutex<BufReader<S>>,
    /// The stream broke, and the agent with it.
    broken: AtomicBool,
    /// How long a tool waits for the user to allow the agent; many clients give up on a tool
    /// after a minute.
    pub answer_timeout: Duration,
    pub answer_poll: Duration,
    /// How long `open_session` waits for the shell of the new terminal.
    pub start_timeout: Duration,
}

impl<S: Read + Write> Remote<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream: Mutex::new(BufReader::new(stream)),
            broken: AtomicBool::new(false),
            answer_timeout: Duration::from_secs(50),
            answer_poll: Duration::from_millis(250),
            start_timeout: Duration::from_secs(5),
        }
    }

    /// Whether the stream broke: Kudari ended, or closed the connection.
    #[must_use]
    pub fn is_broken(&self) -> bool {
        self.broken.load(Ordering::Relaxed)
    }

    /// One request and its reply; the outer error is a stream that broke.
    fn exchange(&self, request: &Request) -> Result<Result<String, Error>, String> {
        let gone = || {
            self.broken.store(true, Ordering::Relaxed);
            "Kudari is not running any more, or closed the connection.".to_string()
        };
        let mut stream = self.stream.lock().unwrap_or_else(PoisonError::into_inner);
        let line = encode_request(request) + "\n";
        stream.get_mut().write_all(line.as_bytes()).map_err(|_| gone())?;
        stream.get_mut().flush().map_err(|_| gone())?;
        let reply = read_line(&mut *stream).map_err(|_| gone())?.ok_or_else(gone)?;
        decode_reply(&reply)
    }

    /// Sends a request again while the user is asked about the agent, until the answer
    /// timeout.
    fn patiently(&self, request: &Request) -> Result<String, String> {
        let start = Instant::now();
        loop {
            match self.exchange(request)? {
                Ok(text) => return Ok(text),
                Err(Error::Waiting) if start.elapsed() < self.answer_timeout => sleep(self.answer_poll),
                Err(error) => return Err(error.message().to_string()),
            }
        }
    }
}

impl<S: Read + Write> Instance for Remote<S> {
    fn terminals(&self) -> Result<String, String> {
        self.patiently(&Request::Terminals)
    }

    fn read_terminal(&self, terminal: i32, lines: u32) -> Result<String, String> {
        self.patiently(&Request::Read { terminal, lines })
    }

    fn send_text(&self, terminal: i32, text: &str, execute: bool) -> Result<String, String> {
        self.patiently(&Request::Type {
            terminal,
            text: text.to_string(),
            execute,
        })
    }

    fn send_key(&self, terminal: i32, key: &str) -> Result<(), String> {
        self.patiently(&Request::Key {
            terminal,
            key: key.to_string(),
        })
        .map(drop)
    }

    /// Returns once the shell of the new terminal runs, so that the next call can use it.
    fn open_session(&self, title: &str) -> Result<String, String> {
        let opened = self.patiently(&Request::Open {
            title: title.to_string(),
        })?;
        let terminal = serde_json::from_str::<Value>(&opened)
            .ok()
            .and_then(|value| value.get("terminal").and_then(Value::as_i64))
            .and_then(|id| i32::try_from(id).ok());
        if let Some(terminal) = terminal {
            let start = Instant::now();
            while matches!(
                self.exchange(&Request::Read { terminal, lines: 1 }),
                Ok(Err(Error::Failed(_)))
            ) && start.elapsed() < self.start_timeout
            {
                sleep(Duration::from_millis(100));
            }
        }
        Ok(opened)
    }

    fn close_session(&self, session: i32) -> Result<(), String> {
        self.patiently(&Request::Close { session }).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn requests_and_replies_survive_the_way() {
        let requests = [
            Request::Terminals,
            Request::Read {
                terminal: -1,
                lines: 20,
            },
            Request::Type {
                terminal: 3,
                text: "ls \"x\"".into(),
                execute: true,
            },
            Request::Key {
                terminal: 3,
                key: "ctrl+c".into(),
            },
            Request::Open { title: "build".into() },
            Request::Close { session: 2 },
        ];
        for request in requests {
            let line = encode_request(&request);
            assert!(!line.contains('\n'));
            assert_eq!(decode_request(&line), Ok(request));
        }
        let replies = [
            Ok("text\nwith lines".to_string()),
            Err(Error::Denied("no".into())),
            Err(Error::Waiting),
            Err(Error::Invalid("wrong".into())),
            Err(Error::Failed("later".into())),
        ];
        for reply in replies {
            let line = encode_reply(&reply);
            assert!(!line.contains('\n'));
            assert_eq!(decode_reply(&line), Ok(reply));
        }
    }

    #[test]
    fn broken_requests_are_refused() {
        assert!(decode_request("no json").is_err());
        assert!(decode_request(r#"{"method": "nothing"}"#).is_err());
        assert!(decode_request(r#"{"method": "read", "terminal": 1}"#).is_err());
        assert!(decode_request(r#"{"method": "read", "terminal": 99999999999, "lines": 1}"#).is_err());
        assert!(decode_request(r#"{"method": "read", "terminal": 1, "lines": -1}"#).is_err());
        assert!(decode_request(r#"{"method": "type", "terminal": 1}"#).is_err());
        assert!(decode_reply("{}").is_err());
        assert!(decode_reply("no json").is_err());
    }

    #[test]
    fn lines_are_read_whole_and_not_too_long() {
        let mut reader = Cursor::new(b"one\r\ntwo".to_vec());
        assert_eq!(read_line(&mut reader).unwrap().as_deref(), Some("one"));
        assert_eq!(read_line(&mut reader).unwrap().as_deref(), Some("two"));
        assert_eq!(read_line(&mut reader).unwrap(), None);
        let mut broken = Cursor::new(vec![0xff, b'\n']);
        assert!(read_line(&mut broken).is_err());
    }

    /// A stream with the replies of Kudari ready, which keeps what is sent.
    struct Script {
        replies: Cursor<Vec<u8>>,
        sent: Vec<u8>,
    }

    impl Read for Script {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.replies.read(buffer)
        }
    }

    impl Write for Script {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.sent.write(buffer)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn remote(replies: &[Result<String, Error>]) -> Remote<Script> {
        let text: String = replies.iter().map(|reply| encode_reply(reply) + "\n").collect();
        let mut remote = Remote::new(Script {
            replies: Cursor::new(text.into_bytes()),
            sent: Vec::new(),
        });
        remote.answer_poll = Duration::ZERO;
        remote.answer_timeout = Duration::from_secs(5);
        remote
    }

    fn sent(remote: Remote<Script>) -> Vec<Request> {
        let script = remote.stream.into_inner().unwrap().into_inner();
        String::from_utf8(script.sent)
            .unwrap()
            .lines()
            .map(|line| decode_request(line).unwrap())
            .collect()
    }

    #[test]
    fn a_request_waits_while_the_user_is_asked() {
        let remote = remote(&[Err(Error::Waiting), Err(Error::Waiting), Ok("{}".into())]);
        assert_eq!(remote.terminals(), Ok("{}".to_string()));
        assert_eq!(
            sent(remote),
            [Request::Terminals, Request::Terminals, Request::Terminals]
        );
    }

    #[test]
    fn refusals_and_a_closed_stream_reach_the_agent() {
        let denied = remote(&[Err(Error::Denied("Reading terminals is switched off.".into()))]);
        assert_eq!(
            denied.read_terminal(0, 5),
            Err("Reading terminals is switched off.".to_string())
        );
        let gone = remote(&[]);
        assert!(gone.send_key(0, "tab").unwrap_err().contains("not running"));
        let mut impatient = remote(&[Err(Error::Waiting), Err(Error::Waiting)]);
        impatient.answer_timeout = Duration::ZERO;
        assert!(impatient.terminals().unwrap_err().contains("asking the user"));
    }

    #[test]
    fn a_new_session_returns_once_its_shell_runs() {
        let remote = remote(&[
            Ok(r#"{"session": 1, "terminal": 2}"#.into()),
            Err(Error::Failed("The terminal is still starting.".into())),
            Ok(String::new()),
            Ok("Typed the text.".into()),
            Ok(String::new()),
        ]);
        assert_eq!(
            remote.open_session("build"),
            Ok(r#"{"session": 1, "terminal": 2}"#.to_string())
        );
        assert_eq!(remote.send_text(2, "ls", false), Ok("Typed the text.".to_string()));
        assert_eq!(remote.close_session(1), Ok(()));
        assert_eq!(
            sent(remote),
            [
                Request::Open { title: "build".into() },
                Request::Read { terminal: 2, lines: 1 },
                Request::Read { terminal: 2, lines: 1 },
                Request::Type {
                    terminal: 2,
                    text: "ls".into(),
                    execute: false,
                },
                Request::Close { session: 1 },
            ]
        );
    }

    #[test]
    fn random_lines_do_no_harm() {
        for input in crate::fuzz::inputs(&[r#"{"method": "read", "terminal": 1, "lines": 2}"#, r#"{"ok": "x"}"#]) {
            let _ = decode_request(&input);
            let _ = decode_reply(&input);
        }
    }
}
