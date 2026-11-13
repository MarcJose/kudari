// SPDX-FileCopyrightText: 2026 Marc Jose <marc@hahn-jose.de>
// SPDX-License-Identifier: MIT

//! `kudari mcp`: a [Model Context Protocol](https://modelcontextprotocol.io) server on
//! standard input and output, through which AI agents use the running Kudari.
//!
//! It holds no state and no rights of its own: every tool is a call to the running
//! instance, where the settings of the user decide what is allowed (see [`crate::agent`]).

use std::io::{BufRead, Write};

use serde_json::{Value, json};

/// The protocol versions this server speaks, the newest first.
const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const INSTRUCTIONS: &str = "Kudari is the drop-down terminal of the user. These \
tools read its terminals and, if the user allows it, type into them. Start with list_terminals. \
The user decides in Kudari's settings whether agents may read and whether they may control \
terminals; when a tool says that something is switched off, tell the user where to switch it \
on instead of trying again. The first tool also makes Kudari ask the user whether this agent \
may use it; the tool waits for the answer. While Kudari asks before running the commands of agents, \
type_text with execute only types the command and the user runs it with Return; read the \
terminal afterwards to see the result. Prefer a session of your own (open_session) to the \
sessions of the user. Terminals can show passwords, tokens and other secrets: do not repeat \
them unless the user asks for it.";

/// The running Kudari, as the tools reach it: over D-Bus on KDE Plasma, a fake in the
/// tests. Errors are sentences for the agent.
#[allow(clippy::missing_errors_doc, reason = "every error is a sentence for the agent")]
pub trait Instance {
    fn terminals(&self) -> Result<String, String>;
    fn read_terminal(&self, terminal: i32, lines: u32) -> Result<String, String>;
    fn send_text(&self, terminal: i32, text: &str, execute: bool) -> Result<String, String>;
    fn send_key(&self, terminal: i32, key: &str) -> Result<(), String>;
    fn open_session(&self, title: &str) -> Result<String, String>;
    fn close_session(&self, session: i32) -> Result<(), String>;
}

/// Answers one JSON-RPC message per line until the input ends.
#[must_use]
pub fn serve(kudari: &impl Instance, input: impl BufRead, mut output: impl Write) -> i32 {
    for line in input.lines() {
        let Ok(line) = line else {
            return 1;
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handle(kudari, &line)
            && writeln!(output, "{reply}").and_then(|()| output.flush()).is_err()
        {
            return 1;
        }
    }
    0
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn failure(id: Value, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// The reply to one message; notifications and responses get none.
#[must_use]
pub fn handle(kudari: &impl Instance, line: &str) -> Option<Value> {
    let Ok(message) = serde_json::from_str::<Value>(line) else {
        return Some(failure(Value::Null, -32700, "Parse error"));
    };
    if !message.is_object() {
        return Some(failure(Value::Null, -32600, "Invalid request"));
    }
    let id = message.get("id").cloned();
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        // This server sends no requests, so a response cannot be meant for it either.
        let response = message.get("result").is_some() || message.get("error").is_some();
        return id
            .filter(|_| !response)
            .map(|id| failure(id, -32600, "Invalid request"));
    };
    let id = id?;
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    Some(match method {
        "initialize" => success(id, initialize(&params)),
        "ping" => success(id, json!({})),
        "tools/list" => success(id, json!({ "tools": tools() })),
        "tools/call" => match call(kudari, &params) {
            Ok(result) => success(id, result),
            Err(message) => failure(id, -32602, &message),
        },
        _ => failure(id, -32601, "Method not found"),
    })
}

fn initialize(params: &Value) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested
        .filter(|version| PROTOCOL_VERSIONS.contains(version))
        .unwrap_or(PROTOCOL_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "kudari", "title": "Kudari", "version": env!("CARGO_PKG_VERSION") },
        "instructions": INSTRUCTIONS,
    })
}

fn terminal_property() -> Value {
    json!({
        "type": "integer",
        "description": "The id of a terminal from list_terminals; the active terminal if left out.",
    })
}

fn tools() -> Value {
    json!([
        {
            "name": "list_terminals",
            "title": "List terminals",
            "description": "Lists the sessions (tabs) of Kudari in order, each with its terminals: \
                id, title, working directory, the program in the foreground, whether one runs \
                (busy), and the size. Needs reading to be allowed.",
            "inputSchema": { "type": "object", "properties": {} },
            "annotations": { "readOnlyHint": true, "openWorldHint": false },
        },
        {
            "name": "read_terminal",
            "title": "Read a terminal",
            "description": "Returns the text of a terminal: its screen, or the last lines of its \
                scrollback and screen. Needs reading to be allowed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "terminal": terminal_property(),
                    "lines": {
                        "type": "integer",
                        "minimum": 0,
                        "maximum": crate::agent::MAX_LINES,
                        "description": "How many lines to read, ending with the screen; 0 or left out reads the screen.",
                    },
                },
            },
            "annotations": { "readOnlyHint": true, "openWorldHint": false },
        },
        {
            "name": "type_text",
            "title": "Type into a terminal",
            "description": "Types one line of text into a terminal, as if the user typed it. With \
                execute, also presses Return to run it; while Kudari asks before running the \
                commands of agents, the user presses Return instead, and typing works only at \
                the shell prompt. Needs control to be allowed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "terminal": terminal_property(),
                    "text": {
                        "type": "string",
                        "maxLength": crate::agent::MAX_TEXT,
                        "description": "One line, without line breaks or other control characters.",
                    },
                    "execute": { "type": "boolean", "description": "Press Return after the text." },
                },
                "required": ["text"],
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": true, "openWorldHint": true },
        },
        {
            "name": "send_key",
            "title": "Press a key",
            "description": "Presses a key in a terminal, such as ctrl+c to stop the running \
                program or the arrow keys in a menu. Needs control to be allowed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "terminal": terminal_property(),
                    "key": { "type": "string", "enum": crate::agent::KEYS },
                },
                "required": ["key"],
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": true, "openWorldHint": false },
        },
        {
            "name": "open_session",
            "title": "Open a session",
            "description": "Opens a new session (tab) with one terminal for your own work, and \
                returns the ids of the session and the terminal once its shell runs. Needs \
                control to be allowed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "The title of the tab." },
                },
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": false, "openWorldHint": false },
        },
        {
            "name": "close_session",
            "title": "Close a session",
            "description": "Closes a session that open_session opened, with the programs in it. \
                The sessions of the user cannot be closed. Needs control to be allowed.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session": { "type": "integer", "description": "The id open_session returned." },
                },
                "required": ["session"],
            },
            "annotations": { "readOnlyHint": false, "destructiveHint": true, "openWorldHint": false },
        },
    ])
}

/// The result of `tools/call`; an error only for a call that names no known tool.
fn call(kudari: &impl Instance, params: &Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("The call names no tool.")?;
    let empty = json!({});
    let arguments = params
        .get("arguments")
        .filter(|value| value.is_object())
        .unwrap_or(&empty);
    let outcome = run_tool(kudari, name, arguments).ok_or_else(|| format!("Unknown tool: {name}"))?;
    let (text, is_error) = match outcome {
        Ok(text) => (text, false),
        Err(text) => (text, true),
    };
    Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
}

fn run_tool(kudari: &impl Instance, name: &str, arguments: &Value) -> Option<Result<String, String>> {
    Some(match name {
        "list_terminals" => kudari.terminals().map(|json| pretty(&json)),
        "read_terminal" => terminal(arguments).and_then(|terminal| {
            let lines = number(arguments, "lines", 0, 0, crate::agent::MAX_LINES.into())?;
            let text = kudari.read_terminal(terminal, u32::try_from(lines).unwrap_or_default())?;
            Ok(if text.is_empty() {
                "The terminal shows no text.".to_string()
            } else {
                text
            })
        }),
        "type_text" => terminal(arguments).and_then(|terminal| {
            let text = arguments
                .get("text")
                .and_then(Value::as_str)
                .ok_or("type_text needs the text to type.")?;
            let execute = arguments.get("execute").and_then(Value::as_bool).unwrap_or(false);
            kudari.send_text(terminal, text, execute)
        }),
        "send_key" => terminal(arguments).and_then(|terminal| {
            let key = arguments
                .get("key")
                .and_then(Value::as_str)
                .ok_or("send_key needs the key to press.")?;
            kudari.send_key(terminal, key).map(|()| format!("Pressed {key}."))
        }),
        "open_session" => {
            let title = arguments.get("title").and_then(Value::as_str).unwrap_or_default();
            kudari.open_session(title).map(|json| pretty(&json))
        }
        "close_session" => number(arguments, "session", -1, 0, i64::from(i32::MAX)).and_then(|session| {
            let session = i32::try_from(session).unwrap_or(-1);
            kudari
                .close_session(session)
                .map(|()| format!("Closed session {session}."))
        }),
        _ => return None,
    })
}

fn pretty(json: &str) -> String {
    serde_json::from_str::<Value>(json)
        .and_then(|value| serde_json::to_string_pretty(&value))
        .unwrap_or_else(|_| json.to_string())
}

/// An integer argument; `fallback` when it is left out, unless that is below `minimum`.
fn number(arguments: &Value, name: &str, fallback: i64, minimum: i64, maximum: i64) -> Result<i64, String> {
    let value = match arguments.get(name) {
        None | Some(Value::Null) if fallback >= minimum => return Ok(fallback),
        None | Some(Value::Null) => return Err(format!("{name} is missing.")),
        Some(value) => value.as_i64(),
    };
    value
        .filter(|value| (minimum..=maximum).contains(value))
        .ok_or_else(|| format!("{name} must be a whole number from {minimum} to {maximum}."))
}

fn terminal(arguments: &Value) -> Result<i32, String> {
    number(arguments, "terminal", -1, -1, i64::from(i32::MAX)).map(|id| i32::try_from(id).unwrap_or(-1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Records the calls and answers like a Kudari with one terminal, 0, in session 0.
    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<String>>,
        denied: bool,
    }

    impl Fake {
        fn answer<T>(&self, call: String, value: T) -> Result<T, String> {
            self.calls.borrow_mut().push(call);
            if self.denied {
                Err("Reading terminals is switched off in Kudari.".to_string())
            } else {
                Ok(value)
            }
        }
    }

    impl Instance for Fake {
        fn terminals(&self) -> Result<String, String> {
            self.answer("terminals".into(), r#"{"sessions":[{"id":0}]}"#.to_string())
        }
        fn read_terminal(&self, terminal: i32, lines: u32) -> Result<String, String> {
            let text = if lines == 7 { String::new() } else { "$ ls".to_string() };
            self.answer(format!("read {terminal} {lines}"), text)
        }
        fn send_text(&self, terminal: i32, text: &str, execute: bool) -> Result<String, String> {
            self.answer(
                format!("text {terminal} {text} {execute}"),
                "Typed the text.".to_string(),
            )
        }
        fn send_key(&self, terminal: i32, key: &str) -> Result<(), String> {
            self.answer(format!("key {terminal} {key}"), ())
        }
        fn open_session(&self, title: &str) -> Result<String, String> {
            self.answer(format!("open {title}"), r#"{"session":1,"terminal":2}"#.to_string())
        }
        fn close_session(&self, session: i32) -> Result<(), String> {
            self.answer(format!("close {session}"), ())
        }
    }

    fn ask(fake: &Fake, message: &Value) -> Value {
        handle(fake, &message.to_string()).expect("a reply")
    }

    fn call_tool(fake: &Fake, name: &str, arguments: &Value) -> (String, bool) {
        let reply = ask(
            fake,
            &json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": name, "arguments": arguments}}),
        );
        let result = &reply["result"];
        (
            result["content"][0]["text"].as_str().unwrap_or_default().to_string(),
            result["isError"].as_bool().unwrap_or_default(),
        )
    }

    #[test]
    fn initialize_agrees_on_a_version() {
        let fake = Fake::default();
        let reply = ask(
            &fake,
            &json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18"}}),
        );
        assert_eq!(reply["id"], 1);
        assert_eq!(reply["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(reply["result"]["serverInfo"]["name"], "kudari");
        assert!(reply["result"]["capabilities"]["tools"].is_object());
        let unknown = ask(
            &fake,
            &json!({"jsonrpc": "2.0", "id": "a", "method": "initialize", "params": {"protocolVersion": "1999-01-01"}}),
        );
        assert_eq!(unknown["result"]["protocolVersion"], PROTOCOL_VERSIONS[0]);
        assert_eq!(unknown["id"], "a");
    }

    #[test]
    fn notifications_and_responses_get_no_reply() {
        let fake = Fake::default();
        assert_eq!(
            handle(&fake, r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
            None
        );
        assert_eq!(handle(&fake, r#"{"jsonrpc":"2.0","id":4,"result":{}}"#), None);
        assert!(fake.calls.borrow().is_empty());
    }

    #[test]
    fn broken_messages_get_errors() {
        let fake = Fake::default();
        assert_eq!(handle(&fake, "{nope").unwrap()["error"]["code"], -32700);
        assert_eq!(handle(&fake, "[1, 2]").unwrap()["error"]["code"], -32600);
        assert_eq!(
            handle(&fake, r#"{"jsonrpc":"2.0","id":5}"#).unwrap()["error"]["code"],
            -32600
        );
        let unknown = ask(&fake, &json!({"jsonrpc": "2.0", "id": 6, "method": "resources/list"}));
        assert_eq!(unknown["error"]["code"], -32601);
        let tool = ask(
            &fake,
            &json!({"jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": {"name": "rm"}}),
        );
        assert_eq!(tool["error"]["code"], -32602);
        let nameless = ask(&fake, &json!({"jsonrpc": "2.0", "id": 8, "method": "tools/call"}));
        assert_eq!(nameless["error"]["code"], -32602);
        assert_eq!(
            ask(&fake, &json!({"jsonrpc": "2.0", "id": 9, "method": "ping"}))["result"],
            json!({})
        );
    }

    #[test]
    fn every_tool_is_listed_with_a_schema() {
        let fake = Fake::default();
        let reply = ask(&fake, &json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}));
        let tools = reply["result"]["tools"].as_array().unwrap();
        let names: Vec<&str> = tools.iter().filter_map(|tool| tool["name"].as_str()).collect();
        assert_eq!(
            names,
            [
                "list_terminals",
                "read_terminal",
                "type_text",
                "send_key",
                "open_session",
                "close_session"
            ]
        );
        for tool in tools {
            assert_eq!(tool["inputSchema"]["type"], "object", "{}", tool["name"]);
            assert!(tool["description"].as_str().is_some_and(|text| text.len() > 40));
            // Each tool exists, so calling it never reports an unknown tool.
            let name = tool["name"].as_str().unwrap();
            assert!(run_tool(&fake, name, &json!({"text": "x", "key": "tab", "session": 1})).is_some());
        }
    }

    #[test]
    fn tools_pass_their_arguments_on() {
        let fake = Fake::default();
        assert!(
            call_tool(&fake, "list_terminals", &json!({}))
                .0
                .contains("\"sessions\"")
        );
        assert_eq!(
            call_tool(&fake, "read_terminal", &json!({})),
            ("$ ls".to_string(), false)
        );
        assert_eq!(
            call_tool(&fake, "read_terminal", &json!({"terminal": 3, "lines": 7})).0,
            "The terminal shows no text."
        );
        call_tool(&fake, "type_text", &json!({"text": "ls", "execute": true}));
        call_tool(&fake, "type_text", &json!({"terminal": 2, "text": "pwd"}));
        assert_eq!(
            call_tool(&fake, "send_key", &json!({"key": "ctrl+c"})).0,
            "Pressed ctrl+c."
        );
        assert!(
            call_tool(&fake, "open_session", &json!({"title": "Build"}))
                .0
                .contains("\"terminal\": 2")
        );
        assert_eq!(
            call_tool(&fake, "close_session", &json!({"session": 1})).0,
            "Closed session 1."
        );
        assert_eq!(
            *fake.calls.borrow(),
            [
                "terminals",
                "read -1 0",
                "read 3 7",
                "text -1 ls true",
                "text 2 pwd false",
                "key -1 ctrl+c",
                "open Build",
                "close 1"
            ]
        );
    }

    #[test]
    fn wrong_arguments_are_tool_errors() {
        let fake = Fake::default();
        for (name, arguments) in [
            ("read_terminal", json!({"terminal": "two"})),
            ("read_terminal", json!({"terminal": -2})),
            ("read_terminal", json!({"lines": 1_000_000})),
            ("type_text", json!({})),
            ("send_key", json!({"terminal": 1})),
            ("close_session", json!({})),
            ("close_session", json!({"session": 1.5})),
        ] {
            let (text, is_error) = call_tool(&fake, name, &arguments);
            assert!(is_error && !text.is_empty(), "{name} {arguments}");
        }
        assert!(fake.calls.borrow().is_empty());
    }

    #[test]
    fn refusals_reach_the_agent() {
        let fake = Fake {
            denied: true,
            ..Fake::default()
        };
        let (text, is_error) = call_tool(&fake, "list_terminals", &json!({}));
        assert!(is_error && text.contains("switched off"));
    }

    #[test]
    fn serve_answers_line_by_line() {
        let fake = Fake::default();
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
            "\n\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#,
            "\n"
        );
        let mut output = Vec::new();
        assert_eq!(serve(&fake, input.as_bytes(), &mut output), 0);
        let lines: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!((lines[0]["id"].clone(), lines[1]["id"].clone()), (json!(1), json!(2)));
    }

    #[test]
    fn pretty_keeps_what_is_no_json() {
        assert_eq!(pretty("not json"), "not json");
        assert_eq!(pretty(r#"{"a":1}"#), "{\n  \"a\": 1\n}");
    }
}
