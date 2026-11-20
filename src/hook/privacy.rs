//! `PreToolUse`: the privacy check on outbound tool calls. A call that names a
//! private path or quotes private text asks; the hit goes to the `asks` log.

use std::borrow::Cow;
use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::privacy::{ChunkHit, Index, Set, find_paths, subtract};
use crate::project::{self, Record};
use crate::store::{Mode, Store, Touch};
use crate::{Env, Error, config, paths};

/// Tools whose input stays in this machine's sessions, or that only read.
const EXEMPT: [&str; 7] = [
    "Read",
    "Glob",
    "Grep",
    "Agent",
    "Skill",
    "TodoWrite",
    "ToolSearch",
];
/// The `asks` insert gives up after this, so a held lock cannot push the hook past
/// its budget. A lost row never changes the decision and is reported on stderr.
const ASKS_BUSY: Duration = Duration::from_millis(100);
/// References counted and listed per call; the rest of a huge input is not worth the work.
const MAX_REFS: usize = 512;
/// The repo searches of the subtraction stop this long after the check starts,
/// leaving room in the 500 ms budget; on timeout every hit stands.
const SUBTRACT_BY: Duration = Duration::from_millis(300);
/// The largest file read as "the edited file's current content".
const MAX_CURRENT: u64 = 16 << 20;

/// Tools that never reach the privacy check, whatever they name.
pub fn exempt_tool(tool: &str) -> bool {
    EXEMPT.contains(&tool) || tool.starts_with("mcp__crook__")
}

/// The reason to ask with, when the call names a private path or quotes private
/// text. Logs the ask. Errors are the caller's to turn into a hook error.
pub fn check(
    env: Env,
    slug: &str,
    session_id: Option<&str>,
    tool: &str,
    input: &Value,
    cwd: &Path,
    rec: &Record,
) -> Result<Option<String>, Error> {
    if exempt_tool(tool) {
        return Ok(None);
    }
    let started = Instant::now();
    let cfg = config::load(env)?;
    let set = Set::load(rec, &cfg, env, slug);
    let write = matches!(tool, "Write" | "Edit");
    let target = write
        .then(|| input["file_path"].as_str())
        .flatten()
        .map(|p| cwd.join(p));
    // `join` keeps an absolute `p` and a relative `cwd` stays relative: not private.
    if let Some(t) = target.as_deref().filter(|t| t.is_absolute())
        && set.contains(t)
    {
        return Ok(None);
    }

    let mut refs: Vec<String> = find_paths(tool, input, cwd, &set)
        .into_iter()
        .map(|h| h.text)
        .collect();

    let dir = paths::project_dir(env, slug)?;
    let index = Index::refresh(&dir, &set)?;
    if let Some(root) = index.capped() {
        let _ = writeln!(
            std::io::stderr(),
            "crook hook: more than {} private files under {}; only the first are indexed",
            crate::privacy::MAX_FILES,
            root.display()
        );
    }
    let text = lookup_text(tool, input);
    let hits = index.lookup(&text);
    if !hits.is_empty() {
        let old = if tool == "Edit" {
            input["old_string"].as_str().unwrap_or_default()
        } else {
            ""
        };
        let current = target.as_deref().map(current_content).unwrap_or_default();
        let public: Vec<String> = rec.links.iter().chain(&rec.tickets).cloned().collect();
        let repos: Vec<PathBuf> = rec.repos.iter().map(PathBuf::from).collect();
        let deadline = started + SUBTRACT_BY;
        let kept = subtract(hits, &public, &current, old, &repos, &set, deadline);
        refs.extend(files_of(kept));
    }

    let refs = distinct(refs);
    let Some(first) = refs.first() else {
        return Ok(None);
    };
    let others = refs.len() - 1;
    log_ask(env, slug, session_id, tool, first, others);
    let mut reason = format!("crook: private reference: {first}");
    if others > 0 {
        reason.push_str(&format!(" (+{others} more)"));
    }
    Ok(Some(reason))
}

/// Drops a reference that sits inside another one: a path found under two
/// spellings of a private root (`/var/..` inside `/private/var/..`) is one
/// reference. Quadratic, so only the first `MAX_REFS` are kept.
fn distinct(mut refs: Vec<String>) -> Vec<String> {
    refs.truncate(MAX_REFS);
    let all = refs.clone();
    refs.retain(|r| {
        !all.iter()
            .any(|o| o.len() > r.len() && o.contains(r.as_str()))
    });
    refs
}

/// One reference per private file, in the order the chunks were found.
fn files_of(hits: Vec<ChunkHit>) -> Vec<String> {
    let mut seen = HashSet::new();
    hits.into_iter()
        .filter(|h| seen.insert(h.file.clone()))
        .map(|h| h.file.display().to_string())
        .collect()
}

/// The text a call sends out: `content` or `new_string` for a file write, every
/// string value of the input otherwise.
fn lookup_text<'a>(tool: &str, input: &'a Value) -> Cow<'a, str> {
    let field = match tool {
        "Write" => Some("content"),
        "Edit" => Some("new_string"),
        _ => None,
    };
    if let Some(f) = field {
        return Cow::Borrowed(input[f].as_str().unwrap_or_default());
    }
    let mut out = String::new();
    let mut stack = vec![input];
    while let Some(v) = stack.pop() {
        match v {
            Value::String(t) => {
                out.push_str(t);
                out.push('\n');
            }
            Value::Array(a) => stack.extend(a.iter().rev()),
            Value::Object(o) => stack.extend(o.values().rev()),
            _ => {}
        }
    }
    Cow::Owned(out)
}

/// The file's text, or nothing when it is missing, large or not UTF-8.
fn current_content(path: &Path) -> String {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() && m.len() <= MAX_CURRENT => {
            std::fs::read_to_string(path).unwrap_or_default()
        }
        _ => String::new(),
    }
}

/// Its own short-lived connection: a lost row never changes the decision.
fn log_ask(env: Env, slug: &str, session: Option<&str>, tool: &str, hit: &str, others: usize) {
    let done = Store::open_with_busy(env, slug, Mode::ReadWrite, ASKS_BUSY).and_then(|mut s| {
        s.write(Touch::empty(), |tx| {
            tx.execute(
                "INSERT INTO asks(at, session_id, tool, hit, \"others\") VALUES (?1, ?2, ?3, ?4, ?5)",
                (
                    project::now(),
                    session,
                    tool,
                    hit,
                    i64::try_from(others).unwrap_or(i64::MAX),
                ),
            )?;
            Ok(())
        })
    });
    if let Err(e) = done {
        let _ = writeln!(
            std::io::stderr(),
            "crook hook: lost asks row (tool {tool}, hit {hit}, +{others}): {e}"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::{Value, json};

    use crate::hook;
    use crate::project::{self, ItemKind};
    use crate::store::{Mode, Store, Touch};

    const QUOTE: &str = "the quarterly migration plan moves billing onto the new ledger";
    const OTHER: &str = "a completely different line about gardening tools and winter storage";

    struct Fx {
        tmp: tempfile::TempDir,
        base: PathBuf,
        notes: PathBuf,
        repo: PathBuf,
    }

    impl Fx {
        fn new() -> Fx {
            let tmp = tempfile::tempdir().unwrap();
            let base = std::fs::canonicalize(tmp.path()).unwrap();
            let notes = base.join("notes");
            let repo = base.join("repo");
            std::fs::create_dir_all(&notes).unwrap();
            std::fs::create_dir_all(&repo).unwrap();
            std::fs::write(notes.join("a"), format!("{QUOTE}\n")).unwrap();
            std::fs::write(notes.join("b"), format!("{OTHER}\n")).unwrap();
            std::fs::write(notes.join("c"), "short\n").unwrap();
            let f = Fx {
                tmp,
                base,
                notes,
                repo,
            };
            project::init(&f.env(), "p").unwrap();
            f.edit(|c| project::add(c, ItemKind::Notes, &f.notes.display().to_string()));
            f
        }

        fn env(&self) -> impl Fn(&str) -> Option<String> + use<> {
            let home = self.base.join("home").display().to_string();
            let state = self.base.join("state").display().to_string();
            let _ = &self.tmp;
            move |k: &str| match k {
                "HOME" => Some(home.clone()),
                "XDG_STATE_HOME" => Some(state.clone()),
                "CROOK_ROLE" => Some("orchestrator".into()),
                "CROOK_PROJECT" => Some("p".into()),
                _ => None,
            }
        }

        fn edit(&self, f: impl FnOnce(&rusqlite::Connection) -> Result<i64, crate::Error>) {
            let mut s = Store::open(&self.env(), "p", Mode::ReadWrite).unwrap();
            s.write(Touch::PROJECT, |tx| f(tx)).unwrap();
        }

        fn pre(&self, tool: &str, input: Value) -> String {
            let e = self.env();
            let v = json!({
                "session_id": "s1", "tool_name": tool, "tool_input": input,
                "cwd": self.repo.display().to_string(),
            });
            let mut out = Vec::new();
            let code = hook::run("PreToolUse", &mut v.to_string().as_bytes(), &e, 1, &mut out);
            assert_eq!(code, 0);
            String::from_utf8(out).unwrap()
        }

        fn asks(&self) -> Vec<(i64, Option<String>, String, String, i64)> {
            let s = Store::open(&self.env(), "p", Mode::ReadOnly).unwrap();
            s.read(|c| {
                let mut q =
                    c.prepare("SELECT at, session_id, tool, hit, others FROM asks ORDER BY id")?;
                let rows = q.query_map([], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .unwrap()
        }

        fn np(&self, name: &str) -> String {
            self.notes.join(name).display().to_string()
        }
    }

    fn reason(out: &str) -> String {
        let v: Value = serde_json::from_str(out).unwrap_or_else(|_| panic!("not JSON: {out:?}"));
        let h = &v["hookSpecificOutput"];
        assert_eq!(h["permissionDecision"], "ask", "{out}");
        h["permissionDecisionReason"].as_str().unwrap().into()
    }

    #[test]
    fn exempt_tools() {
        let f = Fx::new();
        let named = json!({"file_path": f.np("a"), "pattern": f.np("a"), "path": f.np("a")});
        for tool in [
            "Read",
            "Glob",
            "Grep",
            "mcp__crook__status",
            "mcp__crook__spawn",
            "Agent",
            "Skill",
            "TodoWrite",
            "ToolSearch",
        ] {
            assert_eq!(f.pre(tool, named.clone()), "", "{tool}");
        }
        for tool in ["Write", "Edit"] {
            let input = json!({"file_path": f.np("new.md"), "content": QUOTE, "new_string": QUOTE,
                "old_string": "x"});
            assert_eq!(f.pre(tool, input), "", "{tool}");
        }
        assert!(f.asks().is_empty());
    }

    #[test]
    fn write_into_repo_asks() {
        let f = Fx::new();
        let content = format!("# title\n\n{QUOTE}\n");
        let out = f.pre(
            "Write",
            json!({"file_path": f.repo.join("PR.md").display().to_string(), "content": content}),
        );
        assert_eq!(
            reason(&out),
            format!("crook: private reference: {}", f.np("a"))
        );
        let edit = json!({"file_path": f.repo.join("PR.md").display().to_string(),
            "old_string": "x", "new_string": content});
        assert_eq!(
            reason(&f.pre("Edit", edit)),
            format!("crook: private reference: {}", f.np("a"))
        );
        let clean = json!({"file_path": f.repo.join("PR.md").display().to_string(),
            "content": "nothing private here at all, just a handful of ordinary words"});
        assert_eq!(f.pre("Write", clean), "");
    }

    #[test]
    fn public_text_is_subtracted() {
        let f = Fx::new();
        f.edit(|c| project::add(c, ItemKind::Ticket, &format!("T-1: {QUOTE}")));
        let call =
            json!({"file_path": f.repo.join("PR.md").display().to_string(), "content": QUOTE});
        assert_eq!(f.pre("Write", call), "");
        // The edited file already holds the line.
        let target = f.repo.join("keep.md");
        std::fs::write(&target, format!("{OTHER}\n")).unwrap();
        let edit = json!({"file_path": target.display().to_string(), "old_string": OTHER,
            "new_string": OTHER});
        assert_eq!(f.pre("Edit", edit), "");
    }

    #[test]
    fn mcp_and_webfetch_checked() {
        let f = Fx::new();
        let out = f.pre(
            "mcp__linear__save_comment",
            json!({"issue": "T-1", "body": format!("see {}", f.np("c"))}),
        );
        assert_eq!(
            reason(&out),
            format!("crook: private reference: {}", f.np("c"))
        );
        let out = f.pre(
            "WebFetch",
            json!({"url": format!("https://example.com/?f={}", f.np("c")), "prompt": "p"}),
        );
        assert!(
            reason(&out).starts_with("crook: private reference: "),
            "{out}"
        );
        let out = f.pre("mcp__linear__save_comment", json!({"body": "nothing here"}));
        assert_eq!(out, "");
    }

    #[test]
    fn bash_checked() {
        let f = Fx::new();
        let a = f.np("a");
        for cmd in [format!("cat {a}"), format!("cat {a} | gh pr create -F -")] {
            let out = f.pre("Bash", json!({"command": cmd}));
            assert_eq!(
                reason(&out),
                format!("crook: private reference: {a}"),
                "{cmd}"
            );
        }
        let ls = format!("ls {}", f.repo.display());
        assert_eq!(f.pre("Bash", json!({"command": ls})), "");
        let out = f.pre("Monitor", json!({"command": format!("tail -f {a}")}));
        assert!(reason(&out).contains(&a), "{out}");
    }

    #[test]
    fn count_others() {
        let f = Fx::new();
        let cmd = format!("cat {} {} {}", f.np("a"), f.np("b"), f.np("c"));
        let r = reason(&f.pre("Bash", json!({"command": cmd})));
        assert_eq!(
            r,
            format!("crook: private reference: {} (+2 more)", f.np("a"))
        );
    }

    #[test]
    fn asks_row() {
        let f = Fx::new();
        let cmd = format!("cat {} {}", f.np("a"), f.np("b"));
        let before = project::now();
        f.pre("Bash", json!({"command": cmd}));
        let rows = f.asks();
        assert_eq!(rows.len(), 1);
        let (at, session, tool, hit, others) = rows[0].clone();
        assert!(at >= before && at <= project::now());
        assert_eq!(session.as_deref(), Some("s1"));
        assert_eq!(
            (tool.as_str(), hit.as_str(), others),
            ("Bash", f.np("a").as_str(), 1)
        );
        // No ask, no row.
        f.pre("Bash", json!({"command": "ls"}));
        assert_eq!(f.asks().len(), 1);
    }

    #[test]
    fn missing_index() {
        let f = Fx::new();
        let call = json!({"command": format!("echo {QUOTE}")});
        let first = reason(&f.pre("Bash", call.clone()));
        let dir = crate::paths::project_dir(&f.env(), "p")
            .unwrap()
            .join("index");
        assert!(dir.join("index.bin").is_file());
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(reason(&f.pre("Bash", call.clone())), first);
        assert!(dir.join("index.bin").is_file(), "rebuilt");
        std::fs::write(dir.join("index.bin"), b"junk").unwrap();
        assert_eq!(reason(&f.pre("Bash", call)), first);
    }

    #[test]
    fn many_references_still_ask_with_the_first() {
        let f = Fx::new();
        let many = f.notes.join("many");
        std::fs::create_dir_all(&many).unwrap();
        let mut cmd = String::from("cat");
        for i in 0..(super::MAX_REFS + 88) {
            let p = many.join(format!("f{i:04}"));
            std::fs::write(&p, "x\n").unwrap();
            cmd.push_str(&format!(" {}", p.display()));
        }
        let r = reason(&f.pre("Bash", json!({"command": cmd})));
        let first = many.join("f0000").display().to_string();
        assert!(
            r.starts_with(&format!("crook: private reference: {first} (+")),
            "{}",
            &r[..r.len().min(200)]
        );
        assert_eq!(f.asks().len(), 1);
    }

    #[test]
    fn distinct_never_empties_a_list() {
        let same = vec!["/n/a".to_string(), "/n/a".to_string()];
        assert_eq!(
            super::distinct(same).len(),
            2,
            "equal texts are not 'inside' each other"
        );
        assert_eq!(super::distinct(vec!["/n/a".into()]), ["/n/a"]);
        let nested = vec!["/private/n/a".to_string(), "/n/a".to_string()];
        assert_eq!(super::distinct(nested), ["/private/n/a"]);
    }

    #[test]
    fn erroring_check_asks_with_a_hook_error() {
        let f = Fx::new();
        let cfg = crate::paths::config_file(&f.env());
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        for text in ["privat = [\"x\"]\n", "private = ["] {
            std::fs::write(&cfg, text).unwrap();
            let r = reason(&f.pre("Bash", json!({"command": "ls"})));
            assert!(r.starts_with("crook: hook error: config "), "{r}");
        }
        // An unreadable config (a directory in its place) is the same.
        std::fs::remove_file(&cfg).unwrap();
        std::fs::create_dir(&cfg).unwrap();
        let r = reason(&f.pre("Bash", json!({"command": "ls"})));
        assert!(r.starts_with("crook: hook error: config "), "{r}");
        // Exempt tools never reach the config.
        assert_eq!(f.pre("Read", json!({})), "");
    }
}
