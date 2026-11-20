//! Finds the private paths a tool call's input names.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use aho_corasick::AhoCorasick;
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
    s.contains(['*', '?', '[', '{']) || ["@(", "+(", "!("].iter().any(|e| s.contains(e))
}

/// Bash extglob groups (`@(..)`, `+(..)`, `!(..)`, `?(..)`, `*(..)`) with their
/// parentheses turned into braces and `|` into `,`, which the token splitter
/// would otherwise cut at. `$(..)` is left alone. `None` when there is no group.
fn fold_extglob(text: &str) -> Option<String> {
    if !["@(", "+(", "!(", "?(", "*("]
        .iter()
        .any(|e| text.contains(e))
    {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut open: Vec<bool> = Vec::new();
    let mut prev = '\0';
    for c in text.chars() {
        match c {
            '(' => {
                let ext = matches!(prev, '@' | '+' | '!' | '?' | '*');
                open.push(ext);
                out.push(if ext { '{' } else { c });
            }
            ')' => out.push(if open.pop().unwrap_or(false) { '}' } else { c }),
            '|' if open.last() == Some(&true) => out.push(','),
            _ => out.push(c),
        }
        prev = c;
    }
    Some(out)
}

/// The command with shell quotes dropped and `\<char>` unescaped. Detection
/// only, not a lexer: it lets `My\ Notes` and `"$HOME"/notes` read as paths.
fn normalise(cmd: &str) -> String {
    let mut out = String::with_capacity(cmd.len());
    let mut it = cmd.chars();
    while let Some(c) = it.next() {
        match c {
            '\\' => out.extend(it.next()),
            '"' | '\'' => {}
            c => out.push(c),
        }
    }
    out
}

/// Every private path the call's input names, each text once, in document order.
/// `tool` matters for the bare-name rule, which covers the `command` field of
/// Bash and Monitor only: a markdown body with `*` or `[` must not ask.
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
        entries: None,
    };
    // `command` of Bash and Monitor, found by identity so a nested field of the same name is not.
    let command = matches!(tool, "Bash" | "Monitor")
        .then(|| input.get("command").and_then(Value::as_str))
        .flatten();
    enum Item<'v> {
        Val(&'v Value),
        Key(&'v str),
    }
    let mut stack = vec![Item::Val(input)];
    while let Some(item) = stack.pop() {
        match item {
            Item::Key(k) => f.scan(k, false),
            Item::Val(Value::String(t)) => {
                f.scan(t, command.is_some_and(|c| std::ptr::eq(c, t.as_str())));
            }
            Item::Val(Value::Array(a)) => stack.extend(a.iter().rev().map(Item::Val)),
            Item::Val(Value::Object(o)) => {
                for (k, v) in o.iter().rev() {
                    stack.push(Item::Val(v));
                    stack.push(Item::Key(k));
                }
            }
            Item::Val(_) => {}
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
    /// Names in a private `cwd`, read once.
    entries: Option<(Vec<String>, Option<AhoCorasick>)>,
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
        self.pass(text, is_command);
        if is_command {
            let n = normalise(text);
            if n != text {
                self.pass(&n, true);
            }
            if let Some(g) = fold_extglob(&n) {
                self.pass(&g, true);
            }
        }
    }

    fn pass(&mut self, text: &str, is_command: bool) {
        self.roots(text);
        let bare = is_command && self.cwd_private;
        if bare {
            for form in ["$(pwd)", "`pwd`", "$(pwd -P)", "`pwd -P`"] {
                if text.contains(form) {
                    self.push(form, HitKind::Bare);
                }
            }
            self.entry_names(text);
        }
        self.tokens(text, bare, is_command);
    }

    /// A name in the private `cwd` written as a delimited substring, which finds
    /// names with spaces however the shell quoted them.
    fn entry_names(&mut self, text: &str) {
        let cwd = self.cwd;
        let (names, ac) = self.entries.get_or_insert_with(|| {
            let names: Vec<String> = std::fs::read_dir(cwd)
                .map(|d| {
                    d.filter_map(Result::ok)
                        .filter_map(|e| e.file_name().into_string().ok())
                        .collect()
                })
                .unwrap_or_default();
            let ac = AhoCorasick::new(&names).ok();
            (names, ac)
        });
        let mut first: Vec<Option<usize>> = vec![None; names.len()];
        let edge = |c: char, extra: char| ends_path(c) || splits_token(c) || c == '/' || c == extra;
        let delimited = |start: usize, end: usize| {
            let before = text[..start].chars().next_back();
            let after = text[end..].chars().next();
            before.is_none_or(|c| edge(c, '{')) && after.is_none_or(|c| edge(c, '}'))
        };
        if let Some(ac) = ac {
            // One pass over the text for all names; overlapping matches so a short
            // name inside a longer one doesn't hide either.
            for m in ac.find_overlapping_iter(text) {
                let slot = &mut first[m.pattern().as_usize()];
                if slot.is_none() && delimited(m.start(), m.end()) {
                    *slot = Some(m.start());
                }
            }
        } else {
            // The automaton could not be built: scan name by name rather than skip.
            for (slot, name) in first.iter_mut().zip(names.iter()) {
                *slot = text
                    .match_indices(name.as_str())
                    .find(|(s, _)| delimited(*s, s + name.len()))
                    .map(|(s, _)| s);
            }
        }
        let mut found: Vec<(usize, String)> = first
            .iter()
            .zip(names.iter())
            .filter_map(|(at, n)| at.map(|a| (a, n.clone())))
            .collect();
        found.sort();
        for (_, name) in found {
            if !self.s.allowed_with(self.r, &self.cwd.join(&name)) {
                self.push(&name, HitKind::Bare);
            }
        }
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
            for pre in ["${PWD}", "$PWD", "${OLDPWD}", "$OLDPWD", "~+"] {
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
            let full = p.display().to_string();
            let hit = if shell && has_glob_char(&full) {
                // The shell expands the glob, so the directory before it decides.
                self.s.glob_reaches_private(self.r, &full)
            } else {
                self.s.contains_with(self.r, &p) && !self.s.allowed_with(self.r, &p)
            };
            if hit {
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
    fn quoted_and_escaped_spellings() {
        let f = fx();
        fs::create_dir_all(f.home.join("Documents/My Notes")).unwrap();
        fs::create_dir_all(f.home.join("notes")).unwrap();
        let s = f.set(&["~/Documents/My Notes", "~/notes"], &[]);
        for cmd in [
            "cp ~/Documents/My\\ Notes/x.md /tmp",
            "cat \"$HOME\"/notes/plan.md",
            "cat '$HOME'/notes/plan.md",
            "cat \"~/Documents/My Notes/x.md\"",
        ] {
            assert!(!bash(&s, cmd, &f.base).is_empty(), "{cmd}");
        }
        // a private cwd with a name that has a space
        let notes = f.home.join("notes");
        fs::write(notes.join("My Note.md"), "x").unwrap();
        for cmd in [
            "cp \"My Note.md\" /tmp/",
            "cp My\\ Note.md /tmp/",
            "cp 'My Note.md' /tmp/",
        ] {
            assert!(!bash(&s, cmd, &notes).is_empty(), "{cmd}");
        }
        // public cwd: the name alone doesn't hit
        let repo = f.base.join("repo");
        fs::create_dir_all(&repo).unwrap();
        fs::write(repo.join("My Note.md"), "x").unwrap();
        assert!(bash(&s, "cp \"My Note.md\" /tmp/", &repo).is_empty());
    }

    #[test]
    fn braces_and_globs_in_paths() {
        let f = fx();
        let notes = f.home.join("notes");
        fs::create_dir_all(&notes).unwrap();
        fs::write(notes.join("plan.md"), "x").unwrap();
        let s = f.set(&["~/notes"], &[]);
        assert!(!bash(&s, "cp {plan,other}.md /tmp", &notes).is_empty());
        assert!(!bash(&s, "cat ~/{notes,x}/a", &f.base).is_empty());
        assert!(!bash(&s, "cat ~/no*/a.md", &f.base).is_empty());
        assert!(!bash(&s, "cat ~/*/a.md", &f.base).is_empty());
        assert!(!bash(&s, &format!("cat {}/n[o]tes/a", f.home.display()), &f.base).is_empty());
        assert!(bash(&s, "cat ~/zz*/a.md", &f.base).is_empty());
        // braces in a public cwd or outside a command are not a hit
        assert!(bash(&s, "cp {a,b}.md /tmp", &f.base).is_empty());
        assert!(hits(&s, json!({"content": "cat ~/no*/a.md"}), &f.base).is_empty());
    }

    #[test]
    fn pwd_spellings_in_private_cwd() {
        let f = fx();
        let notes = f.home.join("notes");
        fs::create_dir_all(&notes).unwrap();
        let s = f.set(&["~/notes"], &[]);
        for cmd in [
            "cp $(pwd -P)/a /tmp",
            "cp $OLDPWD/a /tmp",
            "cp ${OLDPWD}/a /tmp",
            "cp ~+/a /tmp",
            "cp ~+ /tmp",
        ] {
            assert!(!bash(&s, cmd, &notes).is_empty(), "{cmd}");
            assert!(bash(&s, cmd, &f.base).is_empty(), "public {cmd}");
        }
    }

    #[test]
    fn allow_does_not_follow_a_symlink_out() {
        let f = fx();
        let notes = f.base.join("notes");
        fs::create_dir_all(notes.join("public")).unwrap();
        fs::create_dir_all(notes.join("secret")).unwrap();
        symlink("../secret", notes.join("public/link")).unwrap();
        let rec = Record {
            notes: vec![notes.display().to_string()],
            allow: vec![notes.join("public").display().to_string()],
            ..Record::default()
        };
        let s = f.set_rec(&rec, &Config::default());
        assert!(s.allowed(&notes.join("public/a.md")));
        assert!(!s.allowed(&notes.join("public/link/a.md")));
        let h = hits(
            &s,
            json!({"p": notes.join("public/link/a.md").display().to_string()}),
            &f.base,
        );
        assert!(!h.is_empty());
        assert!(
            hits(
                &s,
                json!({"p": notes.join("public/a.md").display().to_string()}),
                &f.base
            )
            .is_empty()
        );
    }

    #[test]
    fn hits_come_in_document_order() {
        let f = fx();
        let s = f.set(&["/abs/notes"], &[]);
        let input = json!({"a": ["/abs/notes/1", ["/abs/notes/2"], {"k": "/abs/notes/3"}], "b": "/abs/notes/4"});
        let order: Vec<_> = hits(&s, input, &f.base)
            .into_iter()
            .map(|h| h.text)
            .collect();
        assert_eq!(
            order,
            [
                "/abs/notes/1",
                "/abs/notes/2",
                "/abs/notes/3",
                "/abs/notes/4"
            ]
        );
    }

    #[test]
    fn globs_are_never_exempted_by_the_allow_list() {
        let f = fx();
        fs::create_dir_all(f.home.join("notes/public")).unwrap();
        fs::create_dir_all(f.home.join("notes/secret")).unwrap();
        symlink(f.home.join("notes/secret"), f.home.join("notes/public/lnk")).unwrap();
        let s = f.set(&["~/notes"], &["~/notes/public"]);
        for cmd in [
            "cat ~/notes/public/*",
            "cat ~/notes/public/a*.md",
            "cat ~/notes/public/l*/a",
            "cat ~/notes/public/*/a",
            "cat ~/notes/public/.@(.)/secret/a",
            "cat ~/notes/public/.+(.)/secret/a",
            "cat ~/notes/public/!(x)/a",
            "cat ~/notes/*",
            "cat ~/notes/pub*",
        ] {
            assert!(!bash(&s, cmd, &f.base).is_empty(), "{cmd}");
        }
        // a plain path in the allowed directory stays allowed
        assert!(bash(&s, "cat ~/notes/public/a.md", &f.base).is_empty());
    }

    #[test]
    fn allowed_glob_that_climbs_out_still_hits() {
        let f = fx();
        fs::create_dir_all(f.home.join("notes/public")).unwrap();
        fs::create_dir_all(f.home.join("notes/secret")).unwrap();
        let s = f.set(&["~/notes"], &["~/notes/public"]);
        for cmd in [
            "cat ~/notes/public/{..,x}/secret/a",
            "cat ~/notes/public/{a,b}/../../secret",
            "cat ~/notes/public/[.][.]/secret/a",
            "cat ~/notes/public/.*/secret/a",
            "cat ~/notes/public/.?/secret/a",
            "cat ~/notes/public/../secret/*",
        ] {
            assert!(!bash(&s, cmd, &f.base).is_empty(), "{cmd}");
        }
    }

    #[test]
    fn glob_segments_that_can_expand_to_dotdot_are_followed() {
        let f = fx();
        fs::create_dir_all(f.home.join("proj")).unwrap();
        fs::create_dir_all(f.home.join("notes/public")).unwrap();
        let s = f.set(&["~/notes"], &["~/notes/public"]);
        let cwd = f.home.join("proj");
        for cmd in [
            "cat {..,x}/notes/a",
            "cat .@(.)/notes/a",
            "cat .[.]/notes/a",
            "cat ./.?/notes/a",
            "cat ~/proj/.@(.)/notes/a",
            "cat ~/proj/{..,x}/notes/a",
            "cat ~/proj/.@(.)/notes/public/a",
            "cat ~/proj/.@(.|x)/notes/a",
            "cat ~/@(notes)/a",
            "cat ~/proj/a*/../../notes/a",
            "cat ../notes/a",
            "cat ../*/a",
        ] {
            assert!(!bash(&s, cmd, &cwd).is_empty(), "{cmd}");
        }
        for cmd in ["cat {a,b}.txt", "cat ~/proj/*.md", "ls */*"] {
            assert!(bash(&s, cmd, &cwd).is_empty(), "{cmd}");
        }
    }

    /// Run with `cargo nextest run --release --run-ignored only`.
    #[test]
    #[ignore]
    fn timing_entry_names_in_a_large_command() {
        let f = fx();
        let notes = f.home.join("notes");
        fs::create_dir_all(&notes).unwrap();
        for i in 0..2000 {
            fs::write(notes.join(format!("note number {i}.md")), "x").unwrap();
        }
        let s = f.set(&["~/notes"], &[]);
        let body = "word another-word and 'quoted \\ text' ".repeat(140_000);
        assert!(body.len() > 5_000_000);
        let cmd = format!("echo '{body}'");
        let start = std::time::Instant::now();
        let h = bash(&s, &cmd, &notes);
        let took = start.elapsed();
        eprintln!("TIMING entry names, 5 MB command, 2000 entries: {took:?}");
        assert!(took.as_millis() < 500, "{took:?}");
        assert!(h.iter().all(|x| x.kind != HitKind::Bare), "{}", h.len());
        let cmd = format!("{cmd} cp 'note number 1999.md' /tmp");
        assert!(
            bash(&s, &cmd, &notes)
                .iter()
                .any(|x| x.text == "note number 1999.md")
        );
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
