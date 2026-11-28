//! The launch command: one pure function per kind of terminal, so the flag order is tested once
//! (ADR 0007, 0013, 0017, 0018, 0019, 0025; Q1, Q14, Q15, Q18, Q19, Q21, Q32). No other module
//! assembles a `claude` or `hc run` argv.
//!
//! The prompt rides last and must follow a flag that takes exactly one value (ADR 0007, spike 16).
//! `--add-dir` may take several values, so it goes before `--agent`, never right before the prompt.

use std::path::Path;

use crate::hc::{Parent, RunSpec};
use crate::names;

#[cfg(test)]
mod tests;

pub const COORDINATOR_AGENT: &str = "crook:coordinator";
pub const CHILD_AGENT: &str = "crook:child";

/// `--session-id` the first time, `--resume` after that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Session<'a> {
    New(&'a str),
    Resume(&'a str),
}

/// What every launch shares.
#[derive(Clone, Copy, Debug)]
pub struct Common<'a> {
    /// The literal `current/plugin` link path, or the repository's `plugin/` in a debug build
    /// (ADR 0019, spike 18).
    pub plugin_dir: &'a Path,
    /// The `CROOK_BIN` to pass on, from `install::crook_bin` (Q19).
    pub crook_bin: Option<&'a str>,
    pub version: &'a str,
    pub flock_folder: &'a Path,
    /// Passed as a second `--add-dir` only when it is not the flock folder (Q18).
    pub notes_folder: &'a Path,
}

#[derive(Clone, Copy, Debug)]
pub struct Coordinator<'a> {
    pub common: Common<'a>,
    pub slug: &'a str,
    /// The roster's coordinator folder.
    pub cwd: &'a Path,
    pub session: Session<'a>,
    /// `coordinator_model` from the config; `None` runs on the maintainer's default (Q32).
    pub model: Option<&'a str>,
    /// The first task from `crook new --task` (Q1). Used only on a first start.
    pub task: Option<&'a str>,
    /// The literal path from [`paths::crook_call_path`] (ADR 0027).
    pub crook: &'a str,
}

#[derive(Clone, Copy, Debug)]
pub struct Child<'a> {
    pub common: Common<'a>,
    pub slug: &'a str,
    pub name: &'a str,
    pub role: &'a str,
    /// Always set: the model the roster records (Q9).
    pub model: &'a str,
    pub session: Session<'a>,
    pub cwd: &'a Path,
    /// Absolute, for the prompt (Q14).
    pub brief: &'a Path,
    pub result: &'a Path,
    /// The coordinator's terminal (ADR 0020).
    pub parent: &'a str,
}

fn pairs(labels: Vec<String>) -> Vec<(String, String)> {
    labels
        .into_iter()
        .filter_map(|l| {
            l.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect()
}

fn text(path: &Path) -> String {
    path.display().to_string()
}

/// The `--settings` JSON that lets the coordinator call crook without a prompt (ADR 0027). Claude Code
/// matches a rule only against a command with no variables in it, so the rule names the literal path.
pub fn settings_json(crook: &str) -> String {
    serde_json::json!({
        "permissions": { "allow": [format!("Bash(\"{crook}\" *)")] }
    })
    .to_string()
}

/// How the coordinator must call crook, for its first prompt and its SessionStart context (ADR 0027).
pub fn call_rule(crook: &str) -> String {
    format!(
        "Run every crook command as \"{crook}\" <subcommand> ..., with that exact quoted path, as its own Bash \
         command: nothing before it and no pipe, `&&` or `;` after it. Only that form runs without asking the maintainer."
    )
}

/// The argv after `--` in `hc run`.
fn claude_argv(
    common: &Common,
    settings: Option<String>,
    agent: &str,
    model: Option<&str>,
    session: Session,
    prompt: Option<String>,
) -> Vec<String> {
    let mut argv = Vec::new();
    if let Some(bin) = common.crook_bin {
        argv.push("env".to_string());
        argv.push(format!("CROOK_BIN={bin}"));
    }
    argv.extend(["claude".to_string(), "--plugin-dir".to_string()]);
    argv.push(text(common.plugin_dir));
    argv.push("--add-dir".to_string());
    argv.push(text(common.flock_folder));
    if common.notes_folder != common.flock_folder {
        argv.push("--add-dir".to_string());
        argv.push(text(common.notes_folder));
    }
    if let Some(settings) = settings {
        argv.push("--settings".to_string());
        argv.push(settings);
    }
    argv.push("--agent".to_string());
    argv.push(agent.to_string());
    if let Some(model) = model {
        argv.push("--model".to_string());
        argv.push(model.to_string());
    }
    match session {
        Session::New(id) => argv.extend(["--session-id".to_string(), id.to_string()]),
        Session::Resume(id) => argv.extend(["--resume".to_string(), id.to_string()]),
    }
    argv.extend(prompt);
    argv
}

/// `crook open`'s launch: `--no-parent`, so a maintainer's `open` from inside another terminal
/// does not make that terminal its parent (spike 24). A resume has no prompt (ADR 0018).
/// The coordinator always gets `CROOK_BIN` set to `c.crook`, so its hooks name the same path as its allow
/// rule whatever the core's environment holds (ADR 0027).
pub fn coordinator(c: &Coordinator) -> RunSpec {
    let prompt = match c.session {
        Session::New(_) => Some(coordinator_prompt(
            c.slug,
            c.common.flock_folder,
            c.common.notes_folder,
            c.crook,
            c.task,
        )),
        Session::Resume(_) => None,
    };
    let common = Common {
        crook_bin: Some(c.crook),
        ..c.common
    };
    RunSpec {
        name: names::hc_coordinator(c.slug),
        cwd: c.cwd.to_path_buf(),
        labels: pairs(names::coordinator_labels(c.slug, c.common.version)),
        parent: Parent::None,
        argv: claude_argv(
            &common,
            Some(settings_json(c.crook)),
            COORDINATOR_AGENT,
            c.model,
            c.session,
            prompt,
        ),
    }
}

/// `crook spawn` and `crook resume`'s launch of one child. No `--settings`: only `open` adds it.
pub fn child(c: &Child) -> RunSpec {
    let prompt = match c.session {
        Session::New(_) => child_prompt(c.brief, c.result),
        Session::Resume(_) => child_resume_prompt(c.brief, c.result),
    };
    RunSpec {
        name: names::hc_child(c.slug, c.name),
        cwd: c.cwd.to_path_buf(),
        labels: pairs(names::child_labels(c.slug, c.name, c.common.version)),
        parent: Parent::Terminal(c.parent.to_string()),
        argv: claude_argv(
            &c.common,
            None,
            &format!("crook:{}", c.role),
            Some(c.model),
            c.session,
            Some(prompt),
        ),
    }
}

/// Every prompt starts with a fixed word, so it can never be read as a flag.
pub fn coordinator_prompt(
    slug: &str,
    flock_folder: &Path,
    notes_folder: &Path,
    crook: &str,
    task: Option<&str>,
) -> String {
    let mut prompt = format!(
        "You are the coordinator of the crook flock {slug}. Its folder is {} and its notes folder is {}. {} \
         Load the crook-coordinator skill and follow it.",
        flock_folder.display(),
        notes_folder.display(),
        call_rule(crook)
    );
    if let Some(task) = task {
        prompt.push_str("\n\nThe maintainer's first task: ");
        prompt.push_str(task);
    }
    prompt
}

/// The brief and the result path (Q14), and the reporting rule of ADR 0007 and 0024: the result
/// file is the report, and the Stop hook delivers it.
pub fn child_prompt(brief: &Path, result: &Path) -> String {
    format!(
        "Read {}. Write your result to {}. Follow the crook-child skill: create the result file first and \
         keep its status and progress up to date, because your Stop hook reports it to the coordinator.",
        brief.display(),
        result.display()
    )
}

/// ADR 0017's resume prompt, with the paths again (Q14).
pub fn child_resume_prompt(brief: &Path, result: &Path) -> String {
    format!(
        "You were restarted. Check what is already done on disk, then finish without redoing completed work. \
         Your brief is {}. Your result file is {}.",
        brief.display(),
        result.display()
    )
}
