// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The surface programs' Lua runner (`docs/spec/surface-programs.md`): `suhina-lua-runner
//! --control <address> <script.lua>` connects to Suhina's control socket, loads the script into
//! the sandboxed Lua 5.4 environment of [`vm`], and turns the surface-program protocol's
//! messages into handler calls and back ([`suhina_plugins::surface`]).
//!
//! One process runs one script: the process is the boundary, so a crash or a hang of it is what
//! the restart covers, and a script's own error is the error boundary instead — reported once
//! with `script_error`, no handler called after, the process alive for pings and `quit`. The
//! runner is single-threaded: each message is handled to completion before the next is read,
//! and pings are answered between handlers, which is how a wedged handler is seen. While a
//! handler waits on a command's or a read's answer, port messages and states queue, and the
//! wait's pump still answers pings.
//!
//! When the control connection closes, Suhina is gone and the process exits.

mod vm;

use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::io::{BufRead, BufReader, BufWriter, Write as IoWrite};
use std::rc::Rc;

use suhina_plugins::surface::{self, Message};
use suhina_plugins::{connect, trace};

use interprocess::TryClone;
use interprocess::local_socket::Stream;
use interprocess::local_socket::traits::Stream as _;
use mlua::{Function, Lua, MultiValue, Table, Value as LuaValue};

/// Most messages on the runner's send-back queue (spec › Caps); past it `suhina.send` says
/// false and sends nothing.
const SEND_QUEUE_MAX: usize = 4096;
/// Most bytes the send-back queue holds at once.
const SEND_QUEUE_BYTES: usize = 8 * 1024 * 1024;
/// Most messages queued while a handler waits on an answer; past it the runner reports `error`
/// and exits, and the delayed pings are what makes Suhina see the wedge.
const WAIT_QUEUE_MAX: usize = 1024;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (address, script) = match (args.get(1).map(String::as_str), args.get(2), args.get(3)) {
        (Some("--control"), Some(address), Some(script)) => (address.clone(), script.clone()),
        _ => {
            eprintln!("usage: suhina-lua-runner --control <address> <script.lua>");
            std::process::exit(2);
        }
    };
    if let Err(message) = run(&address, &script) {
        eprintln!("suhina-lua-runner: {message}");
        std::process::exit(1);
    }
}

/// An answer that completed a handler's wait.
enum Answer {
    Command { ok: bool, error: Option<String> },
    Read { ok: bool, value: Option<serde_json::Value>, error: Option<String> },
}

/// One run of a script: its handlers, its queues, and the one wait, if a handler is inside
/// `suhina.command` or `suhina.read`.
struct Run {
    inputs: Vec<String>,
    outputs: Vec<String>,
    out: VecDeque<(String, Vec<u8>)>,
    out_bytes: usize,
    subscriptions: HashSet<u64>,
    port_message: Option<Function>,
    state: Option<Function>,
    ports: Option<Function>,
    script_dead: bool,
    next_token: u64,
    /// The wait: its token, and whether it is a command's (`true`) or a read's.
    waiting: Option<(u64, bool)>,
    wait_answer: Option<Answer>,
    queued: VecDeque<Message>,
    /// Messages dropped because the wait queue was full: a control storm outrunning the
    /// answers. The count lands in the trace when it changes, and the last message of a run
    /// stands for its whole storm.
    dropped_while_waiting: usize,
}

impl Run {
    fn new() -> Self {
        Self {
            inputs: Vec::new(),
            outputs: Vec::new(),
            out: VecDeque::new(),
            out_bytes: 0,
            subscriptions: HashSet::new(),
            port_message: None,
            state: None,
            ports: None,
            dropped_while_waiting: 0,
            script_dead: false,
            next_token: 0,
            waiting: None,
            wait_answer: None,
            queued: VecDeque::new(),
        }
    }
}

/// The control socket: the reader and the writer of the one line protocol.
struct Link {
    reader: BufReader<Stream>,
    writer: BufWriter<Stream>,
}

impl Link {
    fn connect(address: &str) -> Result<Self, String> {
        let stream = connect(address).map_err(|e| format!("cannot connect to {address}: {e}"))?;
        let writer = stream.try_clone().map_err(|e| format!("cannot hold the socket's write half: {e}"))?;
        Ok(Self { reader: BufReader::new(stream), writer: BufWriter::with_capacity(64 * 1024, writer) })
    }

    /// Sends one message; the newline is the line's end, and it goes out at once — a peer
    /// waiting on it is not kept waiting on a buffer.
    fn send(&mut self, message: &Message) -> Result<(), String> {
        writeln!(self.writer, "{}", message.encode()).map_err(|e| format!("cannot write: {e}"))?;
        self.writer.flush().map_err(|e| format!("cannot write: {e}"))
    }

    /// The next line, within the protocol's line cap; `None` when the peer closed cleanly.
    fn read_line(&mut self) -> Result<Option<String>, String> {
        let mut line = Vec::new();
        loop {
            let available = self.reader.fill_buf().map_err(|e| format!("cannot read: {e}"))?;
            if available.is_empty() {
                return if line.is_empty() {
                    Ok(None)
                } else {
                    Err("the connection closed in the middle of a line".into())
                };
            }
            match available.iter().position(|b| *b == b'\n') {
                Some(end) => {
                    line.extend_from_slice(&available[..end]);
                    self.reader.consume(end + 1);
                    if line.len() > surface::MAX_LINE {
                        return Err("a line longer than the protocol allows".into());
                    }
                    return String::from_utf8(line).map(Some).map_err(|_| "a line that is not UTF-8 text".into());
                }
                None => {
                    line.extend_from_slice(available);
                    let read = available.len();
                    self.reader.consume(read);
                    if line.len() > surface::MAX_LINE {
                        return Err("a line longer than the protocol allows".into());
                    }
                }
            }
        }
    }

    /// Reads, flushing what is queued first; the one place the runner blocks.
    fn next_line(&mut self, run: &mut Run) -> Result<Option<String>, String> {
        self.flush_queue(run)?;
        self.read_line()
    }

    /// The send-back queue: one `send` line per message, oldest first.
    fn flush_queue(&mut self, run: &mut Run) -> Result<(), String> {
        while let Some((port, bytes)) = run.out.pop_front() {
            run.out_bytes = run.out_bytes.saturating_sub(bytes.len());
            self.send(&Message::Send { port, bytes })?;
        }
        Ok(())
    }

    /// The complete lines already readable, without waiting: what the peer sent while the
    /// handshake and the environment were being made. A partial line stays for the blocking
    /// read, and a line over the cap is refused the way [`Self::read_line`] refuses one.
    fn drain_lines(&mut self) -> Result<Vec<String>, String> {
        self.reader.get_mut().set_nonblocking(true).map_err(|e| format!("cannot make the socket nonblocking: {e}"))?;
        let outcome = self.drain_while_nonblocking();
        self.reader
            .get_mut()
            .set_nonblocking(false)
            .map_err(|e| format!("cannot make the socket blocking again: {e}"))?;
        outcome
    }

    fn drain_while_nonblocking(&mut self) -> Result<Vec<String>, String> {
        let mut lines = Vec::new();
        'drain: loop {
            match self.reader.fill_buf() {
                Ok(available) => {
                    let Some(end) = available.iter().rposition(|b| *b == b'\n') else {
                        // Nothing complete: an empty buffer is a closed connection, and a
                        // partial line is for the blocking read to finish.
                        break Ok(lines);
                    };
                    for line in available[..=end].split(|b| *b == b'\n') {
                        if line.is_empty() {
                            continue;
                        }
                        if line.len() > surface::MAX_LINE {
                            break 'drain Err("a line longer than the protocol allows".into());
                        }
                        match std::str::from_utf8(line) {
                            Ok(text) => lines.push(text.to_string()),
                            Err(_) => break 'drain Err("a line that is not UTF-8 text".into()),
                        }
                    }
                    self.reader.consume(end + 1);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break Ok(lines),
                Err(e) => break Err(format!("cannot read: {e}")),
            }
        }
    }
}

/// What the runner's halves share across its one thread, behind a [`RefCell`].
type Shared<T> = Rc<RefCell<T>>;

fn run(address: &str, script: &str) -> Result<(), String> {
    trace!("lua runner up, connecting");
    let link: Shared<Link> = Rc::new(RefCell::new(Link::connect(address)?));
    // The handshake: one hello each; a peer whose version is not spoken is refused.
    link.borrow_mut().send(&Message::Hello { version: surface::VERSION })?;
    let line = link.borrow_mut().read_line()?.ok_or("the connection closed before the handshake")?;
    match Message::decode(&line) {
        Some(Message::Hello { version }) if version == surface::VERSION => {}
        Some(Message::Hello { version }) => {
            let refusal = format!("version {version} is not spoken; this runner speaks {}", surface::VERSION);
            let _ = link.borrow_mut().send(&Message::Error { message: refusal.clone() });
            return Err(refusal);
        }
        _ => {
            let _ = link.borrow_mut().send(&Message::Error { message: "expected hello".into() });
            return Err("the peer's first message was not hello".into());
        }
    }

    let lua = vm::environment().map_err(|e| format!("cannot make the Lua environment: {e}"))?;
    let runner: Shared<Run> = Rc::new(RefCell::new(Run::new()));
    register_api(&lua, &runner, &link)?;

    // The host sends `ports` right after the handshake — possibly empty — and the script's
    // first lines need it: applied before the script runs, so a chunk that sends sees its
    // ports. Anything else already sent applies too, without waiting.
    let line = link.borrow_mut().read_line()?.ok_or("the connection closed before ports")?;
    match Message::decode(&line) {
        Some(Message::Ports { inputs, outputs }) => {
            let mut runner = runner.borrow_mut();
            runner.inputs = inputs.clone();
            runner.outputs = outputs.clone();
        }
        _ => {
            let _ = link.borrow_mut().send(&Message::Error { message: "expected ports".into() });
            return Err("the host's first message after hello was not ports".into());
        }
    }
    for line in link.borrow_mut().drain_lines()? {
        step(&link, &runner, &lua, &line)?;
    }

    // The script: loaded once, before `ready`; its error is the error boundary, not a crash.
    let outcome = std::fs::read_to_string(script)
        .map_err(|e| format!("cannot read the script: {e}"))
        .and_then(|chunk| lua.load(chunk).set_name(script).exec().map_err(|e| e.to_string()));
    match outcome {
        Ok(()) => {
            // The script's first look at its ports: what the host sent before it loaded, to its
            // `on_ports` handler now, so a script that sends sees its ports before anything
            // else arrives.
            let handler = runner.borrow_mut().ports.clone();
            if let Some(handler) = handler {
                announce_ports(&link, &runner, &lua, handler)?;
            }
            emit(&link, &runner, &Message::Ready)?
        }
        Err(message) => {
            runner.borrow_mut().script_dead = true;
            emit(&link, &runner, &Message::ScriptError { handler: "chunk".into(), message })?;
        }
    }
    trace!("script loaded, ready");

    loop {
        let Some(line) = link.borrow_mut().next_line(&mut runner.borrow_mut())? else {
            return Ok(());
        };
        step(&link, &runner, &lua, &line)?;
        // What queued while a handler waited on its answer runs now, in order.
        loop {
            let next = runner.borrow_mut().queued.pop_front();
            let Some(message) = next else { break };
            step_message(&link, &runner, &lua, &message)?;
        }
    }
}

/// One line's effect on the runner: decoded, answered or queued, and a handler run to
/// completion. An error a handler makes is the error boundary, not a failure of the runner.
fn step(link: &Shared<Link>, runner: &Shared<Run>, lua: &Lua, line: &str) -> Result<(), String> {
    let message = match Message::decode(line) {
        Some(message) => message,
        None => {
            emit(link, runner, &Message::Error { message: "not a message the runner knows".into() })?;
            return Ok(());
        }
    };
    if let Some(outcome) = step_message(link, runner, lua, &message)? {
        // The one way a line completes a wait; the answer goes to the pump by way of Run.
        runner.borrow_mut().wait_answer = Some(outcome);
    }
    Ok(())
}

/// The message's effect; `Some` when it is the answer a waiting handler is after.
fn step_message(
    link: &Shared<Link>,
    runner: &Shared<Run>,
    lua: &Lua,
    message: &Message,
) -> Result<Option<Answer>, String> {
    match message {
        Message::Hello { version } if *version == surface::VERSION => {
            emit(link, runner, &Message::Hello { version: surface::VERSION })?;
        }
        Message::Hello { version } => {
            let refusal = format!("version {version} is not spoken; this runner speaks {}", surface::VERSION);
            let _ = link.borrow_mut().send(&Message::Error { message: refusal.clone() });
            return Err(refusal);
        }
        Message::Ping { id } => emit(link, runner, &Message::Pong { id: *id })?,
        Message::Pong { .. } => {}
        Message::Ports { inputs, outputs } => {
            let call = {
                let mut runner = runner.borrow_mut();
                runner.inputs = inputs.clone();
                runner.outputs = outputs.clone();
                // A handler never runs inside another handler: while one waits on its answer,
                // the ports apply and their handler waits its turn — the message queues, and
                // runs again when it is taken.
                if runner.script_dead || runner.waiting.is_some() { None } else { runner.ports.clone() }
            };
            if let Some(handler) = call {
                call_ports(link, runner, lua, handler, inputs, outputs)?;
            }
        }
        Message::Quit => {
            suhina_plugins::trace!("runner: quit received, exiting");
            std::process::exit(0)
        }
        Message::Error { message } => {
            // Suhina refused something of ours and closed; the report goes to the stderr.
            return Err(format!("Suhina refused a message: {message}"));
        }
        Message::PortMessage { .. } | Message::State { .. } => deliver(link, runner, lua, message)?,
        Message::CommandResult { token, ok, error } => {
            let mut runner = runner.borrow_mut();
            if runner.waiting == Some((*token, true)) {
                runner.waiting = None;
                return Ok(Some(Answer::Command { ok: *ok, error: error.clone() }));
            }
        }
        Message::ReadResult { token, ok, value, error } => {
            let mut runner = runner.borrow_mut();
            if runner.waiting == Some((*token, false)) {
                runner.waiting = None;
                return Ok(Some(Answer::Read { ok: *ok, value: value.clone(), error: error.clone() }));
            }
        }
        // The runner's own ops, arriving where they do not belong.
        Message::Ready
        | Message::Send { .. }
        | Message::Command { .. }
        | Message::Read { .. }
        | Message::Subscribe { .. }
        | Message::Unsubscribe { .. }
        | Message::ScriptError { .. } => {
            emit(link, runner, &Message::Error { message: "not a message for the runner".into() })?;
        }
    }
    Ok(None)
}

/// A port message or a state: the handler runs to completion now, unless another handler is
/// inside a wait, in which case the message queues for the order it will keep.
fn deliver(link: &Shared<Link>, runner: &Shared<Run>, lua: &Lua, message: &Message) -> Result<(), String> {
    enum Decision {
        /// The script is stopped at its error boundary, or the message waits its turn.
        Skip,
        /// The queue of waiting handlers' messages is full: the wedge is reported.
        QueueFull,
        /// The handler to call, and its name for the error boundary's report.
        Call(Option<Function>, &'static str),
    }
    let decision = {
        let mut runner = runner.borrow_mut();
        let decision = if runner.script_dead {
            Decision::Skip
        } else if runner.waiting.is_some() {
            if runner.queued.len() >= WAIT_QUEUE_MAX {
                // A storm of control messages outrunning the answers is not a wedge: the
                // droppable ones are dropped and counted (the way a mapped control's full
                // queue drops), and the last one stands in for its run. The pings are what
                // makes a real wedge — a handler that never returns — visible.
                let droppable = matches!(message, Message::PortMessage { .. } | Message::State { .. });
                if droppable {
                    runner.dropped_while_waiting += 1;
                    Decision::Skip
                } else {
                    Decision::QueueFull
                }
            } else {
                runner.queued.push_back(message.clone());
                Decision::Skip
            }
        } else {
            match message {
                Message::PortMessage { .. } => Decision::Call(runner.port_message.clone(), "port_message"),
                Message::State { .. } => Decision::Call(runner.state.clone(), "state"),
                _ => Decision::Skip,
            }
        };
        let dropped = runner.dropped_while_waiting;
        if dropped % 64 == 1 {
            suhina_plugins::trace!("runner: {} messages dropped while a handler waited", dropped);
        }
        decision
    };
    let (handler, handler_name) = match decision {
        Decision::Skip => return Ok(()),
        Decision::QueueFull => {
            let _ = emit(link, runner, &Message::Error { message: "the runner's queue is full".into() });
            return Err("the queue of messages waiting on a handler's answer is full".into());
        }
        Decision::Call(handler, handler_name) => (handler, handler_name),
    };
    let Some(handler) = handler else { return Ok(()) };
    let arguments: Vec<LuaValue> = match message {
        Message::PortMessage { port, timestamp, bytes } => vec![
            LuaValue::String(lua.create_string(bytes).map_err(|e| e.to_string())?),
            LuaValue::String(lua.create_string(port.as_bytes()).map_err(|e| e.to_string())?),
            LuaValue::Integer((*timestamp).try_into().unwrap_or(i64::MAX)),
        ],
        Message::State { token, value } => {
            vec![
                LuaValue::Integer((*token).try_into().unwrap_or(i64::MAX)),
                vm::json_to_lua(lua, value).map_err(|e| e.to_string())?,
            ]
        }
        _ => return Ok(()),
    };
    match handler.call::<()>(MultiValue::from_vec(arguments)) {
        Ok(()) => Ok(()),
        Err(error) => {
            let message = error.to_string();
            runner.borrow_mut().script_dead = true;
            emit(link, runner, &Message::ScriptError { handler: handler_name.into(), message })?;
            Ok(())
        }
    }
}

/// A protocol result inside a Lua call: the runner's errors are script-visible errors.
fn protocol<T>(result: Result<T, String>) -> mlua::Result<T> {
    result.map_err(mlua::Error::runtime)
}

/// The ports as they are, to the script's `on_ports` handler: two arrays of names, inputs then
/// outputs. A handler's error is the boundary, not a failure of the runner.
fn announce_ports(link: &Shared<Link>, runner: &Shared<Run>, lua: &Lua, handler: Function) -> Result<(), String> {
    let (inputs, outputs) = {
        let runner = runner.borrow_mut();
        (runner.inputs.clone(), runner.outputs.clone())
    };
    call_ports(link, runner, lua, handler, &inputs, &outputs)
}

fn call_ports(
    link: &Shared<Link>,
    runner: &Shared<Run>,
    lua: &Lua,
    handler: Function,
    inputs: &[String],
    outputs: &[String],
) -> Result<(), String> {
    let names = |names: &[String]| -> Result<Table, String> {
        let table = lua.create_table().map_err(|e| e.to_string())?;
        for (at, name) in names.iter().enumerate() {
            table.raw_set(at + 1, lua.create_string(name).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        }
        Ok(table)
    };
    let inputs = names(inputs)?;
    let outputs = names(outputs)?;
    match handler.call::<()>((inputs, outputs)) {
        Ok(()) => Ok(()),
        Err(error) => {
            let message = error.to_string();
            runner.borrow_mut().script_dead = true;
            emit(link, runner, &Message::ScriptError { handler: "ports".into(), message })?;
            Ok(())
        }
    }
}

/// One message from the runner to Suhina: the send-back queue goes first, so the messages
/// leave in the order the script made them.
fn emit(link: &Shared<Link>, runner: &Shared<Run>, message: &Message) -> Result<(), String> {
    link.borrow_mut().flush_queue(&mut runner.borrow_mut())?;
    link.borrow_mut().send(message)
}

/// The `suhina` table: the API the amendment allows, and nothing else. Every member works
/// through the protocol's messages; the send-back queue and the wait are the runner's own.
fn register_api(lua: &Lua, runner: &Shared<Run>, link: &Shared<Link>) -> Result<(), String> {
    let suhina = lua.create_table().map_err(|e| e.to_string())?;
    fn set(table: &mlua::Table, name: &str, value: impl mlua::IntoLua) -> Result<(), String> {
        table.raw_set(name, value).map_err(|e| e.to_string())
    }
    set(&suhina, "version", env!("CARGO_PKG_VERSION"))?;
    let null = lua.named_registry_value::<LuaValue>(vm::NULL_KEY).map_err(|e| e.to_string())?;
    set(&suhina, "null", null)?;
    let midi = vm::midi_table(lua).map_err(|e| e.to_string())?;
    set(&suhina, "midi", midi)?;

    let set_handler = |slot: fn(&mut Run) -> &mut Option<Function>| {
        let runner = Rc::clone(runner);
        lua.create_function(move |_, handler: LuaValue| {
            let handler = match handler {
                LuaValue::Nil => None,
                LuaValue::Function(handler) => Some(handler),
                _ => return Err(mlua::Error::runtime("the handler is a function or nil")),
            };
            *slot(&mut runner.borrow_mut()) = handler;
            Ok(())
        })
        .map_err(|e| e.to_string())
    };
    let port_message_slot: fn(&mut Run) -> &mut Option<Function> = |run| &mut run.port_message;
    let state_slot: fn(&mut Run) -> &mut Option<Function> = |run| &mut run.state;
    let ports_slot: fn(&mut Run) -> &mut Option<Function> = |run| &mut run.ports;
    set(&suhina, "on_port_message", set_handler(port_message_slot)?)?;
    set(&suhina, "on_state", set_handler(state_slot)?)?;
    set(&suhina, "on_ports", set_handler(ports_slot)?)?;

    let send = {
        let runner = Rc::clone(runner);
        lua.create_function(move |_, (port, bytes): (String, mlua::LuaString)| {
            let bytes = bytes.as_bytes().to_vec();
            let mut runner = runner.borrow_mut();
            if !runner.outputs.iter().any(|open| open == &port) {
                return Ok(false);
            }
            if runner.out.len() >= SEND_QUEUE_MAX || runner.out_bytes + bytes.len() > SEND_QUEUE_BYTES {
                return Ok(false);
            }
            runner.out_bytes += bytes.len();
            runner.out.push_back((port, bytes));
            Ok(true)
        })
        .map_err(|e| e.to_string())
    }?;
    set(&suhina, "send", send)?;

    let command = {
        let runner = Rc::clone(runner);
        let link = Rc::clone(link);
        lua.create_function(move |lua, (id, params, gesture): (String, LuaValue, LuaValue)| {
            let params = parameter_json(&params)?;
            let gesture = match gesture {
                LuaValue::Nil => None,
                LuaValue::String(gesture) => Some(
                    gesture.to_str().map_err(|_| mlua::Error::runtime("the gesture is not UTF-8 text"))?.to_string(),
                ),
                _ => return Err(mlua::Error::runtime("the gesture is a string or nil")),
            };
            let token = new_token(&runner);
            runner.borrow_mut().waiting = Some((token, true));
            protocol(emit(&link, &runner, &Message::Command { token, id, params, gesture }))?;
            let (ok, error) = match protocol(pump(&link, &runner, lua))? {
                Answer::Command { ok, error } => (ok, error),
                Answer::Read { .. } => {
                    return Err(mlua::Error::runtime("the answer was another kind's"));
                }
            };
            Ok((ok, error))
        })
        .map_err(|e| e.to_string())
    }?;
    set(&suhina, "command", command)?;

    let read = {
        let runner = Rc::clone(runner);
        let link = Rc::clone(link);
        lua.create_function(move |lua, (id, params): (String, LuaValue)| {
            let params = parameter_json(&params)?;
            let token = new_token(&runner);
            runner.borrow_mut().waiting = Some((token, false));
            protocol(emit(&link, &runner, &Message::Read { token, id, params }))?;
            let (value, error) = match protocol(pump(&link, &runner, lua))? {
                Answer::Read { ok, value, error } => {
                    if ok {
                        let value = value.as_ref().map(|v| vm::json_to_lua(lua, v)).transpose()?;
                        (value.unwrap_or_else(|| null_of(lua)), None)
                    } else {
                        (LuaValue::Nil, error)
                    }
                }
                Answer::Command { .. } => {
                    return Err(mlua::Error::runtime("the answer was another kind's"));
                }
            };
            Ok((value, error))
        })
        .map_err(|e| e.to_string())
    }?;
    set(&suhina, "read", read)?;

    let subscribe = {
        let runner = Rc::clone(runner);
        let link = Rc::clone(link);
        lua.create_function(move |_, (id, params): (String, LuaValue)| {
            let params = parameter_json(&params)?;
            let token = {
                let mut runner = runner.borrow_mut();
                if runner.subscriptions.len() >= surface::MAX_SUBSCRIPTIONS {
                    return Ok((LuaValue::Nil, Some("at most 64 subscriptions".to_string())));
                }
                runner.next_token += 1;
                let token = runner.next_token;
                runner.subscriptions.insert(token);
                token
            };
            protocol(emit(&link, &runner, &Message::Subscribe { token, id, params }))?;
            Ok((LuaValue::Integer(token.try_into().unwrap_or(i64::MAX)), None))
        })
        .map_err(|e| e.to_string())
    }?;
    set(&suhina, "subscribe", subscribe)?;

    let unsubscribe = {
        let runner = Rc::clone(runner);
        let link = Rc::clone(link);
        lua.create_function(move |_, token: u64| {
            runner.borrow_mut().subscriptions.remove(&token);
            protocol(emit(&link, &runner, &Message::Unsubscribe { token }))
        })
        .map_err(|e| e.to_string())
    }?;
    set(&suhina, "unsubscribe", unsubscribe)?;

    lua.globals().set("suhina", suhina).map_err(|e| e.to_string())
}

fn new_token(runner: &Shared<Run>) -> u64 {
    let mut runner = runner.borrow_mut();
    runner.next_token += 1;
    runner.next_token
}

fn null_of(lua: &Lua) -> LuaValue {
    lua.named_registry_value(vm::NULL_KEY).unwrap_or(LuaValue::Nil)
}

/// A call's parameters as protocol JSON: a table an object, nil absent; anything else refused.
fn parameter_json(params: &LuaValue) -> mlua::Result<serde_json::Value> {
    match vm::lua_to_json(params) {
        Ok(value @ (serde_json::Value::Object(_) | serde_json::Value::Null)) => Ok(value),
        Ok(serde_json::Value::Array(_)) => Err(mlua::Error::runtime("the parameters are a table of named values")),
        Ok(_) => Err(mlua::Error::runtime("the parameters are a table or nil")),
        Err(reason) => Err(mlua::Error::runtime(format!("the parameters are not protocol JSON: {reason}"))),
    }
}

/// The loop a handler runs inside `suhina.command` or `suhina.read`: the wait's own answer
/// completes it, pings are answered, and everything else is handled or queued as usual.
fn pump(link: &Shared<Link>, runner: &Shared<Run>, lua: &Lua) -> Result<Answer, String> {
    loop {
        let Some(line) = link.borrow_mut().next_line(&mut runner.borrow_mut())? else {
            return Err("the connection closed while a handler waited on its answer".into());
        };
        step(link, runner, lua, &line)?;
        let answer = runner.borrow_mut().wait_answer.take();
        if let Some(answer) = answer {
            return Ok(answer);
        }
    }
}
