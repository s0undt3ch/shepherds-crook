//! `crook hook <Event>`: the settings hook. With `onFailure: "block"` a crash
//! or a non-zero exit blocks every tool call, so this exits 0 with a defined
//! answer, and exits 2 only when that answer cannot be written: a block is
//! safer than no answer, which lets the call through.

mod output;
mod park;
mod privacy;
pub(crate) mod sessionstart;

use std::io::{Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;

use crate::gate::{self, Entry, Identity, Role};
use crate::project::{self, Record};
use crate::store::{Mode, SCHEMA_VERSION, Store};
use crate::{Env, Error, paths};

const BAD_INPUT: &str = "crook: hook error: bad hook input";
/// Tools that stay available in a broken crook session: they change nothing.
const READ_TOOLS: [&str; 3] = ["Read", "Glob", "Grep"];

#[derive(Deserialize)]
struct Input {
    session_id: Option<String>,
    tool_name: Option<String>,
    #[serde(default)]
    tool_input: Value,
    source: Option<String>,
    cwd: Option<String>,
}

pub fn main(event: &str, stdin: &mut dyn Read, env: Env, out: &mut dyn Write) -> i32 {
    run(event, stdin, env, std::os::unix::process::parent_id(), out)
}

/// `main` with the parent pid injected, so tests control it.
pub fn run(event: &str, stdin: &mut dyn Read, env: Env, ppid: u32, out: &mut dyn Write) -> i32 {
    // stderr only: a panic message must never reach stdout or change the exit code.
    std::panic::set_hook(Box::new(|info| {
        let _ = writeln!(std::io::stderr(), "crook hook: panic: {info}");
    }));
    let text = catch_unwind(AssertUnwindSafe(|| handle(event, stdin, env, ppid)))
        .unwrap_or_else(|_| failure(event, "crook: hook error: internal error"));
    if text.is_empty() {
        return 0;
    }
    match writeln!(out, "{text}").and_then(|()| out.flush()) {
        Ok(()) => 0,
        Err(e) => {
            let _ = writeln!(
                std::io::stderr(),
                "crook hook: cannot write the answer: {e}"
            );
            2
        }
    }
}

/// The defined answer when the hook cannot do its job.
fn failure(event: &str, line: &str) -> String {
    match event {
        "PreToolUse" => output::ask(line),
        "SessionStart" => output::session_context(line),
        _ => String::new(),
    }
}

fn read_input(stdin: &mut dyn Read) -> Option<Input> {
    let mut buf = Vec::new();
    stdin.read_to_end(&mut buf).ok()?;
    serde_json::from_slice(&buf).ok()
}

fn slug_hint(env: Env) -> String {
    env("CROOK_PROJECT")
        .filter(|s| paths::valid_slug(s).is_ok())
        .unwrap_or_default()
}

fn broken_line(env: Env) -> String {
    format!("crook: hook error: run crook open {}", slug_hint(env))
}

fn handle(event: &str, stdin: &mut dyn Read, env: Env, ppid: u32) -> String {
    if !matches!(event, "PreToolUse" | "SessionStart") {
        return String::new();
    }
    let Some(input) = read_input(stdin) else {
        return failure(event, BAD_INPUT);
    };
    #[cfg(feature = "test-panic")]
    if env("CROOK_HOOK_TEST_PANIC").as_deref() == Some("1") {
        #[allow(clippy::panic)]
        {
            panic!("forced panic");
        }
    }
    let id = gate::check(Entry::Hook, env).unwrap_or(Identity {
        slug: String::new(),
        role: Role::None,
        child: None,
    });
    if event == "PreToolUse" {
        pre_tool_use(env, &id, &input)
    } else {
        session_start(env, &id, &input, ppid)
    }
}

fn error_line(e: &Error, env: Env) -> String {
    match e {
        Error::Schema { .. } => broken_line(env),
        e => format!("crook: hook error: {e}"),
    }
}

fn session_start(env: Env, id: &Identity, input: &Input, ppid: u32) -> String {
    let Some(sid) = input.session_id.as_deref().filter(|s| !s.is_empty()) else {
        return output::session_context(BAD_INPUT);
    };
    if id.role == Role::None {
        return output::session_context(&broken_line(env));
    }
    let source = input.source.as_deref().unwrap_or("startup");
    match sessionstart::handle(env, id, sid, source, ppid) {
        Ok(card) => output::session_context(&card),
        Err(e) => output::session_context(&error_line(&e, env)),
    }
}

/// Broken sessions fail closed for everything but the read-only tools.
fn broken_pre(env: Env, tool: &str) -> String {
    if READ_TOOLS.contains(&tool) {
        String::new()
    } else {
        output::ask(&broken_line(env))
    }
}

fn pre_tool_use(env: Env, id: &Identity, input: &Input) -> String {
    let Some(tool) = input.tool_name.as_deref().filter(|t| !t.is_empty()) else {
        return output::ask(BAD_INPUT);
    };
    if id.role == Role::None {
        return broken_pre(env, tool);
    }
    // Never migrates, never creates, and waits at most the store's busy timeout.
    let store = match Store::open(env, &id.slug, Mode::ReadOnly) {
        Ok(s) => s,
        Err(Error::Schema { .. }) => return broken_pre(env, tool),
        Err(e) => return output::ask(&error_line(&e, env)),
    };
    // A read-only open takes any version, so compare it here.
    let step = store.read(|c| {
        let v: i32 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v != SCHEMA_VERSION {
            return Err(Error::Schema {
                slug: id.slug.clone(),
            });
        }
        match park::check(c, id, input.session_id.as_deref(), tool, &input.tool_input)? {
            Some(d) => Ok(Step::Decided(d)),
            None if privacy::exempt_tool(tool, &input.tool_input) => Ok(Step::Done),
            None => Ok(Step::Privacy(Box::new(project::get(c)?))),
        }
    });
    drop(store);
    let result = step.and_then(|step| match step {
        Step::Decided(d) => Ok(Some(d)),
        Step::Done => Ok(None),
        Step::Privacy(rec) => {
            let cwd = match &input.cwd {
                Some(c) => PathBuf::from(c),
                None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")),
            };
            let sid = input.session_id.as_deref();
            privacy::check(env, &id.slug, sid, tool, &input.tool_input, &cwd, &rec)
                .map(|r| r.map(park::Decision::Ask))
        }
    });
    match result {
        Ok(None) => String::new(),
        Ok(Some(park::Decision::Deny(r))) => output::deny(&r),
        Ok(Some(park::Decision::Ask(r))) => output::ask(&r),
        Err(Error::Schema { .. }) => broken_pre(env, tool),
        Err(e) => output::ask(&error_line(&e, env)),
    }
}

/// What the read-only phase of `PreToolUse` settled.
enum Step {
    Decided(park::Decision),
    Done,
    /// The privacy check is next, over this project record.
    Privacy(Box<Record>),
}

#[cfg(test)]
mod tests;
