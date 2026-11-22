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
fn quoting_a_private_line_without_a_path_does_not_ask() {
    // The check covers private paths only; the protocol tells sessions not to
    // quote private material.
    let f = Fx::new();
    let pr = f.repo.join("PR.md").display().to_string();
    let content = format!("# title\n\n{QUOTE}\n");
    assert_eq!(
        f.pre("Write", json!({"file_path": pr, "content": content})),
        ""
    );
    let edit = json!({"file_path": pr, "old_string": "x", "new_string": content});
    assert_eq!(f.pre("Edit", edit), "");
    let echo = json!({"command": format!("echo {QUOTE}")});
    assert_eq!(f.pre("Bash", echo), "");
    assert!(f.asks().is_empty());
}

#[test]
fn write_naming_a_private_path_asks() {
    let f = Fx::new();
    let pr = f.repo.join("PR.md").display().to_string();
    let content = format!("# title\n\nsee {}\n", f.np("a"));
    assert_eq!(
        reason(&f.pre("Write", json!({"file_path": pr, "content": content}))),
        format!("crook: private reference: {}", f.np("a"))
    );
}

#[test]
fn write_through_a_symlink_out_of_notes_is_checked() {
    let f = Fx::new();
    std::os::unix::fs::symlink(&f.repo, f.notes.join("repolink")).unwrap();
    let target = f.notes.join("repolink/PR.md").display().to_string();
    let content = format!("see {}", f.np("a"));
    let out = f.pre("Write", json!({"file_path": target, "content": content}));
    assert!(
        reason(&out).starts_with("crook: private reference: "),
        "{out}"
    );
    let edit = json!({"file_path": target, "old_string": "x", "new_string": content});
    assert!(reason(&f.pre("Edit", edit)).starts_with("crook: private reference: "));
    let nb = json!({"notebook_path": f.notes.join("repolink/n.ipynb").display().to_string(),
        "new_source": content});
    assert!(reason(&f.pre("NotebookEdit", nb)).starts_with("crook: private reference: "));
    // A link from the repo into the notes resolves inside them: still exempt.
    std::os::unix::fs::symlink(&f.notes, f.repo.join("notelink")).unwrap();
    let into = f.repo.join("notelink/new.md").display().to_string();
    assert_eq!(
        f.pre("Write", json!({"file_path": into, "content": content})),
        ""
    );
    let nb = json!({"notebook_path": f.np("n.ipynb"), "new_source": content});
    assert_eq!(f.pre("NotebookEdit", nb), "");
}

#[test]
fn agent_exemption_is_for_local_built_in_types() {
    let f = Fx::new();
    let prompt = format!("summarise {}", f.np("a"));
    for input in [
        json!({"prompt": prompt, "subagent_type": "llmtrim-grok"}),
        json!({"prompt": prompt, "subagent_type": "understand-anything:file-analyzer"}),
        json!({"prompt": prompt, "subagent_type": "general-purpose", "isolation": "remote"}),
        json!({"prompt": prompt, "isolation": "remote"}),
        json!({"prompt": prompt, "subagent_type": 7}),
        json!({"prompt": prompt, "isolation": ["remote"]}),
        json!({"prompt": prompt, "subagent_type": "Explore", "isolation": "cloud"}),
    ] {
        assert_eq!(
            reason(&f.pre("Agent", input.clone())),
            format!("crook: private reference: {}", f.np("a")),
            "{input}"
        );
    }
    for input in [
        json!({"prompt": prompt}),
        json!({"prompt": prompt, "subagent_type": "Explore"}),
        json!({"prompt": prompt, "subagent_type": "fork", "isolation": "worktree"}),
        json!({"prompt": prompt, "subagent_type": "Plan", "isolation": "worktree"}),
    ] {
        assert_eq!(f.pre("Agent", input.clone()), "", "{input}");
    }
}

#[test]
fn encoded_paths_ask() {
    let f = Fx::new();
    let enc = f.np("c").replace('/', "%2F");
    let out = f.pre(
        "WebFetch",
        json!({"url": format!("https://example.com/?f={enc}"), "prompt": "p"}),
    );
    assert!(
        reason(&out).starts_with("crook: private reference: "),
        "{out}"
    );
    let enc = f.np("c").replace('/', "%2f");
    let out = f.pre("mcp__x__fetch", json!({"q": format!("x={enc}&y=1")}));
    assert!(reason(&out).contains(&f.np("c")), "{out}");
}

#[test]
fn relative_private_entry_is_a_hook_error() {
    let f = Fx::new();
    // A legacy entry, from before `add` refused relative ones.
    f.edit(|c| {
        c.execute(
            "INSERT INTO project_item(kind, value, pos) VALUES ('private', 'rel/notes', 99)",
            [],
        )?;
        Ok(0)
    });
    assert_eq!(
        reason(&f.pre("Bash", json!({"command": "ls"}))),
        "crook: hook error: private entry is not absolute: rel/notes"
    );
    assert_eq!(f.pre("Read", json!({})), "");
}

#[test]
fn relative_config_private_entry_is_a_hook_error() {
    let f = Fx::new();
    let cfg = crate::paths::config_file(&f.env());
    std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    std::fs::write(&cfg, "private = [\"~/vault\", \"$HOME/v\", \"vault/x\"]\n").unwrap();
    assert_eq!(
        reason(&f.pre("Bash", json!({"command": "ls"}))),
        "crook: hook error: private entry is not absolute: vault/x"
    );
    std::fs::write(&cfg, "private = [\"~/vault\", \"${HOME}/v\", \"~\"]\n").unwrap();
    assert_eq!(f.pre("Bash", json!({"command": "ls"})), "");
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
fn asks_wait_fits_the_budget() {
    use std::time::{Duration, Instant};
    let t = Instant::now();
    assert_eq!(super::asks_wait(t, t), Some(super::ASKS_BUSY));
    let late = t + Duration::from_millis(430);
    assert_eq!(super::asks_wait(t, late), Some(Duration::from_millis(50)));
    assert_eq!(super::asks_wait(t, t + Duration::from_millis(480)), None);
    assert_eq!(super::asks_wait(t, t + Duration::from_secs(2)), None);
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
