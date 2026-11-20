//! Finds the private paths a tool call's input names.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::Set;
use super::set::Resolver;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitKind {
    Root,
    Token,
    Bare,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub text: String,
    pub kind: HitKind,
}

/// Characters that end a path inside a longer string.
fn ends_path(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '\'' | '"' | '`' | '<' | '>' | '|' | ';' | '&' | '(' | ')' | ','
        )
}

/// Characters that split a token into candidates, besides `ends_path`.
fn splits_token(c: char) -> bool {
    matches!(c, '=' | '@' | ',' | ':')
}

fn has_glob_char(s: &str) -> bool {
    s.contains(['*', '?', '['])
}

/// Every private path the call's input names, each text once. `tool` matters
/// for the bare-name rule, which covers the `command` field of Bash and Monitor
/// only: a markdown body with `*` or `[` must not ask.
pub fn find_paths(tool: &str, input: &Value, cwd: &Path, s: &Set) -> Vec<Hit> {
    let r = Resolver::default();
    let mut f = Finder {
        s,
        r: &r,
        cwd,
        cwd_private: cwd.is_absolute() && s.contains_with(&r, cwd),
        hits: Vec::new(),
        seen: HashSet::new(),
        tokens: HashSet::new(),
    };
    // `command` of Bash and Monitor, found by identity so a nested field of the same name is not.
    let command = matches!(tool, "Bash" | "Monitor")
        .then(|| input.get("command").and_then(Value::as_str))
        .flatten();
    let mut stack = vec![input];
    while let Some(v) = stack.pop() {
        match v {
            Value::String(t) => f.scan(t, command.is_some_and(|c| std::ptr::eq(c, t.as_str()))),
            Value::Array(a) => stack.extend(a),
            Value::Object(o) => {
                for (k, v) in o {
                    f.scan(k, false);
                    stack.push(v);
                }
            }
            _ => {}
        }
    }
    f.hits
}

struct Finder<'a> {
    s: &'a Set,
    r: &'a Resolver,
    cwd: &'a Path,
    cwd_private: bool,
    hits: Vec<Hit>,
    seen: HashSet<String>,
    /// Tokens already judged, so a repeated one costs nothing.
    tokens: HashSet<(String, bool)>,
}

impl Finder<'_> {
    fn push(&mut self, text: &str, kind: HitKind) {
        if self.seen.insert(text.to_string()) {
            self.hits.push(Hit {
                text: text.to_string(),
                kind,
            });
        }
    }

    fn scan(&mut self, text: &str, is_command: bool) {
        self.roots(text);
        if is_command && self.cwd_private {
            for form in ["$(pwd)", "`pwd`"] {
                if text.contains(form) {
                    self.push(form, HitKind::Bare);
                }
            }
        }
        self.tokens(text, is_command && self.cwd_private, is_command);
    }

    /// Root pass: each spelling of each private root, where the next character
    /// ends the root.
    fn roots(&mut self, text: &str) {
        for form in self.s.forms() {
            for (start, _) in text.match_indices(form.as_str()) {
                let end = start + form.len();
                let next = text[end..].chars().next();
                if !next.is_none_or(|c| c == '/' || ends_path(c)) {
                    continue;
                }
                let stop = text[end..].find(ends_path).map_or(text.len(), |i| end + i);
                let cand = &text[start..stop];
                if self.is_allowed_text(cand) {
                    continue;
                }
                self.push(cand, HitKind::Root);
            }
        }
    }

    fn is_allowed_text(&self, cand: &str) -> bool {
        match self.abs(cand, false) {
            Some(p) => self.s.allowed_with(self.r, &p),
            None => false,
        }
    }

    /// An absolute path for a token that names one, or a relative path with a slash.
    fn abs(&self, tok: &str, shell: bool) -> Option<PathBuf> {
        if let Some(p) = self.s.expand_home(tok) {
            return Some(p);
        }
        if shell {
            for pre in ["${PWD}", "$PWD"] {
                if let Some(rest) = tok.strip_prefix(pre) {
                    if rest.is_empty() {
                        return Some(self.cwd.to_path_buf());
                    }
                    if let Some(rel) = rest.strip_prefix('/') {
                        return Some(self.cwd.join(rel));
                    }
                }
            }
        }
        if tok.starts_with('/') {
            Some(PathBuf::from(tok))
        } else if tok.contains('/') && self.cwd.is_absolute() {
            Some(self.cwd.join(tok))
        } else {
            None
        }
    }

    fn tokens(&mut self, text: &str, bare: bool, shell: bool) {
        for run in text
            .split(|c: char| ends_path(c) && c != ',')
            .filter(|r| !r.is_empty())
        {
            self.token(run, bare, shell);
            if run.contains(splits_token) {
                for part in run.split(splits_token).filter(|p| !p.is_empty()) {
                    self.token(part, bare, shell);
                }
            }
        }
    }

    fn token(&mut self, tok: &str, bare: bool, shell: bool) {
        if !self.tokens.insert((tok.to_string(), shell)) {
            return;
        }
        if let Some(p) = self.abs(tok, shell) {
            if self.s.contains_with(self.r, &p) && !self.s.allowed_with(self.r, &p) {
                self.push(tok, HitKind::Token);
            }
            return;
        }
        if !bare {
            return;
        }
        let joined = self.cwd.join(tok);
        let hit = if tok == "." || has_glob_char(tok) {
            true
        } else {
            !tok.starts_with('$')
                && joined.symlink_metadata().is_ok()
                && !self.s.allowed_with(self.r, &joined)
        };
        if hit {
            self.push(tok, HitKind::Bare);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};

    use serde_json::{Value, json};
    use tempfile::TempDir;

    use super::*;
    use crate::config::Config;
    use crate::project::Record;

    struct Fx {
        _t: TempDir,
        base: PathBuf,
        home: PathBuf,
    }

    /// A temp tree with a real `home` and a `real` directory, `link` pointing at it.
    fn fx() -> Fx {
        let t = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(t.path()).unwrap();
        fs::create_dir_all(base.join("home")).unwrap();
        fs::create_dir_all(base.join("real")).unwrap();
        symlink(base.join("real"), base.join("link")).unwrap();
        Fx {
            home: base.join("home"),
            base,
            _t: t,
        }
    }

    impl Fx {
        fn set(&self, private: &[&str], allow: &[&str]) -> Set {
            let rec = Record {
                private: private.iter().map(|s| s.to_string()).collect(),
                allow: allow.iter().map(|s| s.to_string()).collect(),
                ..Record::default()
            };
            self.set_rec(&rec, &Config::default())
        }

        fn set_rec(&self, rec: &Record, cfg: &Config) -> Set {
            let h = self.home.display().to_string();
            let env = move |k: &str| (k == "HOME").then(|| h.clone());
            Set::load(rec, cfg, &env, "demo")
        }

        fn p(&self, rel: &str) -> String {
            self.base.join(rel).display().to_string()
        }
    }

    fn hits(s: &Set, input: Value, cwd: &Path) -> Vec<Hit> {
        find_paths("Write", &input, cwd, s)
    }

    fn bash(s: &Set, cmd: &str, cwd: &Path) -> Vec<Hit> {
        find_paths("Bash", &json!({ "command": cmd }), cwd, s)
    }

    #[test]
    fn symlink_realpath() {
        let f = fx();
        fs::create_dir_all(f.base.join("real/notes")).unwrap();
        let cwd = &f.base;
        // private written through the link, input through the real path
        let s = f.set(&[&f.p("link/notes")], &[]);
        assert!(!hits(&s, json!({"file_path": f.p("real/notes/a.md")}), cwd).is_empty());
        // and the reverse
        let s = f.set(&[&f.p("real/notes")], &[]);
        assert!(!hits(&s, json!({"file_path": f.p("link/notes/a.md")}), cwd).is_empty());
        // a file that exists, too
        fs::write(f.base.join("real/notes/b.md"), "x").unwrap();
        assert!(!hits(&s, json!({"file_path": f.p("link/notes/b.md")}), cwd).is_empty());
        // `~` expands
        fs::create_dir_all(f.home.join("notes")).unwrap();
        let s = f.set(&["~/notes", "~/kb/**/*.md", "/abs/vault"], &[]);
        assert!(!hits(&s, json!({"p": "~/notes/x"}), cwd).is_empty());
        let s = f.set(&[&f.p("home/notes")], &[]);
        assert!(!hits(&s, json!({"p": "~/notes/x"}), cwd).is_empty());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn symlink_realpath_private_tmp() {
        let t = tempfile::Builder::new().tempdir_in("/tmp").unwrap();
        let name = t.path().file_name().unwrap().to_str().unwrap().to_string();
        fs::create_dir(t.path().join("notes")).unwrap();
        let f = fx();
        let short = format!("/tmp/{name}/notes");
        let long = format!("/private/tmp/{name}/notes");
        let s = f.set(&[&short], &[]);
        assert!(!hits(&s, json!({"p": format!("{long}/a.md")}), &f.base).is_empty());
        let s = f.set(&[&long], &[]);
        assert!(!hits(&s, json!({"p": format!("{short}/a.md")}), &f.base).is_empty());
        // embedded with a space nearby, either spelling
        assert!(!hits(&s, json!({"p": format!("x {short}/a.md y")}), &f.base).is_empty());
    }

    #[test]
    fn relative_resolves() {
        let f = fx();
        fs::create_dir_all(f.base.join("r/repo")).unwrap();
        fs::create_dir_all(f.base.join("r/notes")).unwrap();
        let s = f.set(&[&f.p("r/notes")], &[]);
        let cwd = f.base.join("r/repo");
        assert!(!hits(&s, json!({"file_path": "../notes/a.md"}), &cwd).is_empty());
        assert!(hits(&s, json!({"file_path": "../repo/a.md"}), &cwd).is_empty());
    }

    #[test]
    fn globs() {
        let f = fx();
        fs::create_dir_all(f.home.join("kb/a/b")).unwrap();
        let s = f.set(&["~/kb/**/secret-*.md"], &[]);
        let cwd = &f.base;
        assert!(!hits(&s, json!({"p": "~/kb/a/b/secret-1.md"}), cwd).is_empty());
        assert!(hits(&s, json!({"p": "~/kb/a/b/public.md"}), cwd).is_empty());
        // a glob written through a symlink, a file reached the real way
        fs::create_dir_all(f.base.join("real/kb/x")).unwrap();
        let s = f.set(&[&f.p("link/kb/**/secret-*.md")], &[]);
        assert!(!hits(&s, json!({"p": f.p("real/kb/x/secret-2.md")}), cwd).is_empty());
        let s = f.set(&[&f.p("real/kb/**/secret-*.md")], &[]);
        assert!(!hits(&s, json!({"p": f.p("link/kb/x/secret-2.md")}), cwd).is_empty());
        assert!(hits(&s, json!({"p": f.p("link/kb/x/other.md")}), cwd).is_empty());
        // `*` stays in its directory
        let s = f.set(&["~/kb/*.md"], &[]);
        assert!(!hits(&s, json!({"p": "~/kb/a.md"}), cwd).is_empty());
        assert!(hits(&s, json!({"p": "~/kb/a/b.md"}), cwd).is_empty());
    }

    #[test]
    fn invalid_private_glob_falls_back_to_its_directory() {
        let f = fx();
        let s = f.set(&["~/kb/[a-"], &[]);
        assert!(!hits(&s, json!({"p": "~/kb/zzz"}), &f.base).is_empty());
    }

    #[test]
    fn allow_glob_stays_in_dir() {
        let f = fx();
        fs::create_dir_all(f.home.join("x/sub")).unwrap();
        let s = f.set(&["~/x"], &["~/x/*.md"]);
        let cwd = &f.base;
        assert!(s.allowed(&f.home.join("x/a.md")));
        assert!(!s.allowed(&f.home.join("x/sub/y.md")));
        assert!(hits(&s, json!({"p": "~/x/a.md"}), cwd).is_empty());
        assert!(!hits(&s, json!({"p": "~/x/sub/y.md"}), cwd).is_empty());
    }

    #[test]
    fn allow_list() {
        let f = fx();
        let rec = Record {
            notes: vec![f.p("notes")],
            allow: vec![f.p("notes/public")],
            ..Record::default()
        };
        let cfg = Config {
            private: vec![],
            allow: vec![f.p("notes/shared.md")],
        };
        let s = f.set_rec(&rec, &cfg);
        let cwd = &f.base;
        assert!(hits(&s, json!({"p": f.p("notes/public/a.md")}), cwd).is_empty());
        assert!(hits(&s, json!({"p": f.p("notes/shared.md")}), cwd).is_empty());
        // embedded, allowed
        assert!(
            hits(
                &s,
                json!({"p": format!("--f={}", f.p("notes/shared.md"))}),
                cwd
            )
            .is_empty()
        );
        assert!(!hits(&s, json!({"p": f.p("notes/secret.md")}), cwd).is_empty());
        // a notes folder is private without being listed in `private`
        assert!(s.contains(&f.base.join("notes/x")));
        assert!(!s.contains(&f.base.join("other")));
    }

    #[test]
    fn user_wide_private_applies() {
        let f = fx();
        let cfg = Config {
            private: vec!["~/vault".into()],
            allow: vec![],
        };
        let s = f.set_rec(&Record::default(), &cfg);
        assert!(!hits(&s, json!({"p": "~/vault/a"}), &f.base).is_empty());
    }

    #[test]
    fn nested_json() {
        let f = fx();
        let s = f.set(&["/abs/notes"], &[]);
        let input = json!({
            "a": [1, {"b": ["x", {"c": "/abs/notes/deep.md"}]}],
            "d": {"e": {"f": [["/abs/notes/arr.md"]]}},
            "n": null, "t": true,
        });
        let h = hits(&s, input, &f.base);
        assert!(h.iter().any(|x| x.text.contains("deep.md")), "{h:?}");
        assert!(h.iter().any(|x| x.text.contains("arr.md")), "{h:?}");
    }

    #[test]
    fn embedded_forms() {
        let f = fx();
        let s = f.set(&["/abs/notes", "~/Documents/My Notes", "~/notes"], &[]);
        let cwd = &f.base;
        for text in [
            "--body-file=/abs/notes/p.md",
            "file=@/abs/notes/a",
            "https://x/?f=/abs/notes/a",
            "a,/abs/notes/b",
            "cp 'x' ~/Documents/My Notes/x.md /tmp/out",
            "see ~/Documents/My Notes/x.md, then",
            "(~/Documents/My Notes)",
            "--body-file \"$HOME/notes/p.md\"",
            "--body-file ${HOME}/notes/p.md",
            "--body-file \"${HOME}/notes/p.md\"",
            "cat $HOME/notes",
        ] {
            assert!(
                !hits(&s, json!({ "command": text }), cwd).is_empty(),
                "{text}"
            );
        }
        // a root with a space, written with the real home path
        let spaced = format!("x {}/Documents/My Notes/y.md z", f.home.display());
        assert!(!hits(&s, json!({ "command": spaced }), cwd).is_empty());
    }

    #[test]
    fn root_boundary() {
        let f = fx();
        let s = f.set(&["/abs/notes", "~/Documents/My Notes"], &[]);
        let cwd = &f.base;
        assert!(hits(&s, json!({"p": "/abs/notes-old/a"}), cwd).is_empty());
        assert!(hits(&s, json!({"p": "x=/abs/notesy"}), cwd).is_empty());
        assert!(hits(&s, json!({"p": "~/Documents/My Notes-old/a"}), cwd).is_empty());
        assert!(!hits(&s, json!({"p": "/abs/notes"}), cwd).is_empty());
        assert!(!hits(&s, json!({"p": "'/abs/notes'"}), cwd).is_empty());
    }

    #[test]
    fn bare_names_in_private_cwd() {
        let f = fx();
        let notes = f.base.join("abs/notes");
        fs::create_dir_all(&notes).unwrap();
        fs::write(notes.join("plan.md"), "x").unwrap();
        let s = f.set(&[&f.p("abs/notes")], &[]);
        for cmd in [
            "cp plan.md ~/repo/docs/plan.md",
            "gh pr create --body-file plan.md",
            "curl -F file=@plan.md",
            "tar c .",
            "cat *.md",
            "gh pr create --body-file \"$PWD/plan.md\"",
            "gh pr create --body-file ${PWD}/plan.md",
            "cp \"$(pwd)/plan.md\" /tmp/",
            "cp `pwd`/plan.md /tmp/",
        ] {
            assert!(!bash(&s, cmd, &notes).is_empty(), "{cmd}");
            let m = find_paths("Monitor", &json!({ "command": cmd }), &notes, &s);
            assert!(!m.is_empty(), "Monitor {cmd}");
        }
        // names that don't exist, in a private cwd, don't count
        assert!(bash(&s, "echo hello world", &notes).is_empty());
        // public cwd: the same file name doesn't hit
        let repo = f.base.join("abs/repo");
        fs::create_dir_all(&repo).unwrap();
        fs::write(repo.join("plan.md"), "x").unwrap();
        assert!(bash(&s, "cp plan.md x", &repo).is_empty());
        assert!(bash(&s, "cat *.md", &repo).is_empty());
        // only Bash and Monitor's `command`
        let w = find_paths(
            "Write",
            &json!({ "content": "see [plan](x) and *.md", "command": "cat plan.md" }),
            &notes,
            &s,
        );
        assert!(w.is_empty(), "{w:?}");
        let b = find_paths(
            "Bash",
            &json!({ "description": "cat plan.md *.md" }),
            &notes,
            &s,
        );
        assert!(b.is_empty(), "{b:?}");
        // the known gap: a bare git push stays quiet
        assert!(bash(&s, "git push", &notes).is_empty());
    }

    #[test]
    fn many_hits_are_listed_once_each() {
        let f = fx();
        let s = f.set(&["/abs/notes"], &[]);
        let h = hits(
            &s,
            json!({"a": "/abs/notes/a /abs/notes/a /abs/notes/b"}),
            &f.base,
        );
        let mut texts: Vec<_> = h.iter().map(|x| x.text.clone()).collect();
        let n = texts.len();
        texts.sort();
        texts.dedup();
        assert_eq!(n, texts.len(), "{h:?}");
    }

    #[test]
    fn dotdot_follows_the_kernel_through_symlinks() {
        let f = fx();
        fs::create_dir_all(f.base.join("real/sub/deep")).unwrap();
        fs::create_dir_all(f.base.join("real/sub/secret")).unwrap();
        symlink(f.base.join("real/sub/deep"), f.base.join("lnk")).unwrap();
        let s = f.set(&[&f.p("real/sub/secret")], &[]);
        // lexically `lnk/..` is the base; the kernel goes to `real/sub`
        let h = hits(&s, json!({"p": f.p("lnk/../secret/a.md")}), &f.base);
        assert!(!h.is_empty());
        // a `..` in the missing tail folds lexically
        let h = hits(
            &s,
            json!({"p": f.p("real/sub/secret/new/../a.md")}),
            &f.base,
        );
        assert!(!h.is_empty());
    }
}
