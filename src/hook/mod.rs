//! `crook hook <event>`: the plugin's three hooks (ADR 0008, 0009, 0019, 0020).
//!
//! A hook must never break the session. It always exits 0, writes problems to stderr only, catches
//! panics and has an overall time budget below the timeout `hooks.json` sets (Q25, Q42). A Stop hook
//! never prints a `block` decision. Only `session-start` and `user-prompt-submit` print anything,
//! one `hookSpecificOutput` JSON line with `additionalContext`.
//!
//! No hook starts an hc core: every hc call but `--version` would start one with the hook's own
//! Claude variables (fixtures README). A hook runs only in a terminal of a live core, so it returns
//! before any hc call unless `$HC_TERMINAL_ID` is set and the core's socket (`$HC_TERMINAL_SOCKET`,
//! which hc gives every terminal) accepts a connection. A stale socket file left by a dead core is
//! refused, so the hook skips instead of waking a new core. Preflight's version and schema checks are
//! skipped too, since they would cost two more hc calls on every turn (Q25).

use std::io::{Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::cmd::Io;
use crate::hc::Hc;
use crate::hc::client::HcClient;
use crate::run::Budgeted;
use crate::{Ctx, Env, install, names};

pub mod prompt;
pub mod session_start;
pub mod stop;
pub mod version;

#[cfg(test)]
mod tests;

/// Below the 120 and 30 second timeouts in `plugin/hooks/hooks.json` (Q25, Q42).
pub const START_BUDGET: Duration = Duration::from_secs(100);
pub const OTHER_BUDGET: Duration = Duration::from_secs(20);

/// Claude Code drops an `additionalContext` longer than this.
const MAX_CONTEXT_CHARS: usize = 10_000;

/// Input is a few hundred bytes; a bigger one is not read further.
const MAX_INPUT_BYTES: u64 = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    SessionStart,
    Stop,
    UserPromptSubmit,
}

impl Event {
    /// The names `plugin/hooks/hooks.json` passes.
    pub fn parse(name: &str) -> Option<Event> {
        match name {
            "session-start" => Some(Event::SessionStart),
            "stop" => Some(Event::Stop),
            "user-prompt-submit" => Some(Event::UserPromptSubmit),
            _ => None,
        }
    }

    pub fn budget(self) -> Duration {
        match self {
            Event::SessionStart => START_BUDGET,
            Event::Stop | Event::UserPromptSubmit => OTHER_BUDGET,
        }
    }

    /// Claude Code's own name for the event, for `hookEventName`.
    fn claude_name(self) -> &'static str {
        match self {
            Event::SessionStart => "SessionStart",
            Event::Stop => "Stop",
            Event::UserPromptSubmit => "UserPromptSubmit",
        }
    }
}

/// The fields of Claude Code's hook input crook reads. Anything else is ignored.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Input {
    pub session_id: Option<String>,
    pub source: Option<String>,
    pub cwd: Option<String>,
}

fn parse_input(text: &str) -> Result<Input, String> {
    let value: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
    let Some(object) = value.as_object() else {
        return Err("not a JSON object".to_string());
    };
    let field = |key: &str| {
        object
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .filter(|s| !s.is_empty())
    };
    Ok(Input {
        session_id: field("session_id"),
        source: field("source"),
        cwd: field("cwd"),
    })
}

/// The crook terminal a hook runs in, from its hc labels (Q4): a coordinator has no `child`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Who {
    pub address: String,
    pub flock: String,
    pub child: Option<String>,
    /// The parent terminal's address; a child's is its coordinator.
    pub parent: Option<String>,
}

/// `None` for a terminal hc does not know or that has no `crook.flock` label: not a crook session,
/// so the hook does nothing.
fn identify(hc: &dyn Hc, io: &mut Io, id: &str) -> Option<Who> {
    match hc.get(id) {
        Ok(Some(t)) => {
            let flock = t.labels.get(names::LABEL_FLOCK)?.clone();
            Some(Who {
                address: t.address,
                flock,
                child: t.labels.get(names::LABEL_CHILD).cloned(),
                parent: t.parent,
            })
        }
        Ok(None) => None,
        Err(e) => {
            io.warn(format!("crook hook: could not look up terminal {id}: {e}"));
            None
        }
    }
}

/// What a hook has to say: free lines, and results the coordinator has not seen yet. Those are marked
/// delivered only once the line carrying them has been written (and fits the cap), so a failed write
/// leaves them for the next rescan.
#[derive(Debug, Default)]
pub struct Reply {
    pub lines: Vec<String>,
    pub undelivered: Vec<prompt::Undelivered>,
}

/// What the event does.
fn handle(
    ctx: &Ctx,
    hc: &dyn Hc,
    io: &mut Io,
    event: Event,
    input: &Input,
    terminal: &str,
) -> Reply {
    #[cfg(feature = "test-panic")]
    if (ctx.env)("CROOK_HOOK_TEST_PANIC").as_deref() == Some("1") {
        #[allow(clippy::panic)]
        {
            panic!("forced panic");
        }
    }
    let Some(who) = identify(hc, io, terminal) else {
        return Reply::default();
    };
    match event {
        Event::SessionStart => session_start::run(ctx, hc, io, input, &who),
        Event::Stop => stop::run(ctx, hc, io, input, &who),
        Event::UserPromptSubmit => prompt::run(ctx, hc, io, input, &who),
    }
}

fn say(err: &mut dyn Write, line: impl std::fmt::Display) {
    let _ = writeln!(err, "crook hook: {line}");
    let _ = err.flush();
}

/// Runs one hook. Never fails and never panics: every problem is a line on `err`. `core_up` says
/// whether an hc core is there to talk to; it is asked only once the hook has a terminal.
pub fn run(
    ctx: &Ctx,
    hc: &dyn Hc,
    core_up: &dyn Fn() -> bool,
    event_name: &str,
    stdin: &mut dyn Read,
    out: &mut dyn Write,
    err: &mut dyn Write,
) {
    let Some(event) = Event::parse(event_name) else {
        say(err, format!("unknown event {event_name:?}, doing nothing"));
        return;
    };
    let mut text = String::new();
    if let Err(e) = stdin.take(MAX_INPUT_BYTES).read_to_string(&mut text) {
        say(err, format!("could not read the hook input: {e}"));
        return;
    }
    let input = match parse_input(&text) {
        Ok(i) => i,
        Err(why) => {
            say(err, format!("unusable hook input ({why}), doing nothing"));
            return;
        }
    };
    let Some(terminal) = (ctx.env)("HC_TERMINAL_ID").filter(|v| !v.is_empty()) else {
        return;
    };
    if !core_up() {
        say(
            err,
            "no hc core answers on $HC_TERMINAL_SOCKET, doing nothing",
        );
        return;
    }
    let mut unused = Vec::new();
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let mut io = Io {
            out: &mut unused,
            err: &mut *err,
        };
        handle(ctx, hc, &mut io, event, &input, &terminal)
    }));
    match outcome {
        Ok(reply) => emit(out, err, event, reply),
        // The panic hook has already said what happened.
        Err(_) => say(
            err,
            "a panic ended the hook early; the session is not affected",
        ),
    }
}

/// The text for `additionalContext` and how many of the undelivered results fit under the cap. The
/// free lines come first; a result line that would pass the cap is left out, not cut.
fn fit(lines: &[String], undelivered: &[prompt::Undelivered]) -> (String, usize) {
    let mut text: String = lines.join("\n").chars().take(MAX_CONTEXT_CHARS).collect();
    let mut used = text.chars().count();
    let mut fitted = 0;
    for item in undelivered {
        let line = item.line();
        let sep = usize::from(used > 0);
        let add = sep + line.chars().count();
        if used + add > MAX_CONTEXT_CHARS {
            break;
        }
        if sep == 1 {
            text.push('\n');
        }
        text.push_str(&line);
        used += add;
        fitted += 1;
    }
    (text, fitted)
}

fn emit(out: &mut dyn Write, err: &mut dyn Write, event: Event, reply: Reply) {
    if event == Event::Stop || (reply.lines.is_empty() && reply.undelivered.is_empty()) {
        return;
    }
    let (text, fitted) = fit(&reply.lines, &reply.undelivered);
    let doc = json!({
        "hookSpecificOutput": {
            "hookEventName": event.claude_name(),
            "additionalContext": text,
        }
    });
    if let Err(e) = writeln!(out, "{doc}").and_then(|()| out.flush()) {
        say(err, format!("could not write the answer: {e}"));
        return;
    }
    let delivered: Vec<&prompt::Undelivered> = reply.undelivered.iter().take(fitted).collect();
    let mut sink = Vec::new();
    prompt::commit(
        &delivered,
        &mut Io {
            out: &mut sink,
            err,
        },
    );
}

/// Whether the core `$HC_TERMINAL_SOCKET` names accepts a connection.
pub fn core_reachable(env: Env) -> bool {
    env("HC_TERMINAL_SOCKET")
        .filter(|v| !v.is_empty())
        .is_some_and(|path| std::os::unix::net::UnixStream::connect(path).is_ok())
}

/// Runs `on_expiry` unless the returned guard is dropped within `limit`.
pub fn watchdog(limit: Duration, on_expiry: impl FnOnce() + Send + 'static) -> mpsc::Sender<()> {
    let (guard, rx) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        if rx.recv_timeout(limit) == Err(mpsc::RecvTimeoutError::Timeout) {
            on_expiry();
        }
    });
    guard
}

/// Runs a hook under its time budget. The budget caps every hc call (each gets the time left, at
/// most its own timeout), and when it runs out the watchdog ends the call in flight, says so on
/// stderr and then runs `after_expiry`, which for the real process exits 0. `install::refresh` runs
/// inside the same net, since it can lock and write too.
#[allow(clippy::too_many_arguments)]
pub fn run_budgeted(
    ctx: &Ctx,
    hc_program: &str,
    event: &str,
    budget: Duration,
    stdin: &mut dyn Read,
    out: &mut dyn Write,
    err: &mut dyn Write,
    after_expiry: impl FnOnce() + Send + 'static,
) {
    let runner = Budgeted::new(Instant::now() + budget);
    let killer = runner.killer();
    let _guard = watchdog(budget, move || {
        killer.kill();
        let _ = writeln!(
            std::io::stderr(),
            "crook hook: over its time budget, giving up"
        );
        after_expiry();
    });
    let ctx = Ctx {
        env: ctx.env,
        vars: ctx.vars,
        runner: &runner,
        clock: ctx.clock,
        current_exe: ctx.current_exe,
        new_id: ctx.new_id,
    };
    match catch_unwind(AssertUnwindSafe(|| install::refresh(&ctx))) {
        Ok(warnings) => {
            for warning in warnings {
                let _ = writeln!(err, "{warning}");
            }
        }
        Err(_) => say(err, "a panic ended the link update early"),
    }
    let hc = HcClient::new(&runner).with_program(hc_program.to_string());
    run(
        &ctx,
        &hc,
        &|| core_reachable(ctx.env),
        event,
        stdin,
        out,
        err,
    );
}

/// The process entry for `crook hook <event>`: the real hc, the real streams, the time budget. The
/// caller exits 0 whatever happens here.
pub fn main(ctx: &Ctx, event: &str) {
    std::panic::set_hook(Box::new(|info| {
        let _ = writeln!(std::io::stderr(), "crook hook: panic: {info}");
    }));
    // The streams are not locked for the whole run, so the watchdog can still write to stderr while
    // the hook is stuck in a subprocess.
    run_budgeted(
        ctx,
        "hc",
        event,
        Event::parse(event).map_or(OTHER_BUDGET, Event::budget),
        &mut std::io::stdin(),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
        || std::process::exit(0),
    );
}
