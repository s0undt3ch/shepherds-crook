//! Finds the private paths a tool call's input names.

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

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

/// Percent-decodes `text`, with `+` read as a space first when `plus` is set, so
/// `%2B` stays a plus sign. A `%` without two hex digits after it stays as it is.
fn percent_decode(text: &str, plus: bool) -> String {
    let hex = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    };
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let pair = match (bytes.get(i + 1), bytes.get(i + 2)) {
            (Some(&h), Some(&l)) if b == b'%' => hex(h).zip(hex(l)),
            _ => None,
        };
        match pair {
            Some((h, l)) => {
                out.push(h << 4 | l);
                i += 3;
            }
            None => {
                out.push(if plus && b == b'+' { b' ' } else { b });
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The whitespace-separated runs of `text` that decoding changes, decoded, one
/// per line; empty when there are none.
fn decoded_runs(text: &str, plus: bool) -> String {
    let mut out = String::new();
    for run in text
        .split(char::is_whitespace)
        .filter(|r| r.contains(['%', '+']))
    {
        let d = percent_decode(run, plus);
        if d != run {
            out.push_str(&d);
            out.push('\n');
        }
    }
    out
}

/// macOS volumes (APFS by default) are case-insensitive, so root spellings
/// match in any case there. Only ASCII is folded, which keeps byte offsets.
const FOLD_CASE: bool = cfg!(target_os = "macos");

fn fold(text: &str) -> Cow<'_, str> {
    if FOLD_CASE {
        Cow::Owned(text.to_ascii_lowercase())
    } else {
        Cow::Borrowed(text)
    }
}

/// A string as one value: trimmed, out of one pair of matching quotes, with
/// any leading `./` dropped, so `"./My Notes"` and `'notes'` are the bare root.
fn whole(text: &str) -> &str {
    let mut t = text.trim();
    for q in ['"', '\''] {
        if let Some(inner) = t.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            t = inner.trim();
            break;
        }
    }
    while let Some(rest) = t.strip_prefix("./") {
        t = rest;
    }
    t
}

/// Whether the character after a root spelling ends it: a separator, a path
/// end, or a `.` or `:` that closes a sentence (`see My Notes.`).
fn ends_root(rest: &str) -> bool {
    let mut it = rest.chars();
    match it.next() {
        None => true,
        Some('/' | '\\') => true,
        Some('.' | ':') => it.next().is_none_or(ends_path),
        Some(c) => ends_path(c),
    }
}

/// A root's last path component with whitespace, and the component before it,
/// both case-folded on macOS.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Spaced {
    parent: Option<String>,
    name: String,
}

/// The `Spaced` name of each directory that has one: what the root pass looks
/// for when a root's fixed spellings miss.
fn spaced_names<'p>(dirs: impl Iterator<Item = &'p PathBuf>) -> Vec<Spaced> {
    let mut out: Vec<Spaced> = dirs
        .filter_map(|d| {
            let comps: Vec<Component> = d.components().collect();
            let i = comps.iter().rposition(|c| {
                c.as_os_str()
                    .to_str()
                    .is_some_and(|c| c.contains(char::is_whitespace))
            })?;
            let parent = match comps.get(i.wrapping_sub(1)) {
                Some(Component::Normal(p)) => p.to_str().map(|p| fold(p).into_owned()),
                _ => None,
            };
            let name = fold(comps[i].as_os_str().to_str()?).into_owned();
            Some(Spaced { parent, name })
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Where a path that reaches byte `at` of `text` may start, nearest first: after
/// each space or token separator going left, until a line break, a quote or
/// other path end, or the start of the text. Bounded, since a path has few
/// spaces.
fn path_starts(text: &str, at: usize) -> Vec<usize> {
    const MAX: usize = 8;
    let mut out = Vec::new();
    for (i, c) in text[..at].char_indices().rev() {
        if (c.is_whitespace() && !matches!(c, '\n' | '\r')) || splits_token(c) {
            out.push(i + c.len_utf8());
        } else if ends_path(c) {
            out.push(i + c.len_utf8());
            return out;
        }
        if out.len() >= MAX {
            return out;
        }
    }
    out.push(0);
    out
}

/// `to` written relative to `from`, both absolute. `None` when `to` is `from`
/// or above it, which would be only `..` steps.
fn relative(from: &Path, to: &Path) -> Option<String> {
    let from: Vec<Component> = from.components().collect();
    let to: Vec<Component> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if common == to.len() {
        return None;
    }
    let mut out = PathBuf::new();
    for _ in common..from.len() {
        out.push("..");
    }
    out.extend(&to[common..]);
    Some(out.display().to_string())
}

/// Every private path the call's input names, each text once, in document order.
/// `tool` matters for the bare-name rule, which covers the `command` field of
/// Bash and Monitor only: a markdown body with `*` or `[` must not ask.
pub fn find_paths(tool: &str, input: &Value, cwd: &Path, s: &Set) -> Vec<Hit> {
    let r = Resolver::default();
    let mut f = Finder::new(s, &r, cwd, None);
    f.walk(tool, input);
    f.hits
}

/// `find_paths` that gives up at `deadline`: `None` when the input was too large
/// to check in time.
pub fn find_paths_by(
    tool: &str,
    input: &Value,
    cwd: &Path,
    s: &Set,
    deadline: Instant,
) -> Option<Vec<Hit>> {
    let r = Resolver::default();
    let mut f = Finder::new(s, &r, cwd, Some(deadline));
    f.walk(tool, input);
    (!f.late).then_some(f.hits)
}

impl<'a> Finder<'a> {
    fn new(s: &'a Set, r: &'a Resolver, cwd: &'a Path, deadline: Option<Instant>) -> Self {
        let cwd_private = cwd.is_absolute() && s.contains_with(r, cwd);
        let mut rel_forms = Vec::new();
        if cwd.is_absolute() {
            for c in r.variants(cwd) {
                let dirs = s.roots().iter().chain(s.glob_dirs());
                rel_forms.extend(dirs.filter_map(|dir| relative(&c, dir)));
            }
        }
        let mut rel_forms: Vec<String> = rel_forms.iter().map(|f| fold(f).into_owned()).collect();
        rel_forms.sort();
        rel_forms.dedup();
        let mut root_forms: Vec<String> = (s.forms().iter().chain(s.glob_forms()))
            .map(|f| fold(f).into_owned())
            .collect();
        root_forms.sort();
        root_forms.dedup();
        let spaced = spaced_names(s.roots().iter().chain(s.glob_dirs()));
        Finder {
            s,
            r,
            cwd,
            cwd_private,
            root_forms,
            rel_forms,
            spaced,
            deadline,
            late: false,
            ticks: 0,
            hits: Vec::new(),
            seen: HashSet::new(),
            tokens: HashSet::new(),
            entries: None,
        }
    }

    /// Whether the deadline has passed, looking at the clock every few calls.
    fn past_deadline(&mut self) -> bool {
        if self.late {
            return true;
        }
        self.ticks = self.ticks.wrapping_add(1);
        if self.ticks.is_multiple_of(64)
            && let Some(d) = self.deadline
        {
            self.late = Instant::now() >= d;
        }
        self.late
    }

    fn walk(&mut self, tool: &str, input: &Value) {
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
                Item::Key(k) => self.scan(k, false),
                Item::Val(Value::String(t)) => {
                    self.scan(t, command.is_some_and(|c| std::ptr::eq(c, t.as_str())));
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
            if self.late {
                return;
            }
        }
    }
}

struct Finder<'a> {
    s: &'a Set,
    r: &'a Resolver,
    cwd: &'a Path,
    cwd_private: bool,
    /// Spellings of the private roots and glob directories, case-folded on macOS.
    root_forms: Vec<String>,
    /// The same written relative to `cwd`, case-folded on macOS.
    rel_forms: Vec<String>,
    /// The last component with a space of each root and glob directory.
    spaced: Vec<Spaced>,
    deadline: Option<Instant>,
    /// The deadline passed: the hits are incomplete.
    late: bool,
    ticks: u32,
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

    /// Every string once as written, then the runs in it with a `%` or `+`
    /// decoded as a URL would be: only those runs, so a large input with a few
    /// `+` signs is not scanned twice.
    fn scan(&mut self, text: &str, is_command: bool) {
        self.scan_as_written(text, is_command);
        if text.contains(['%', '+']) {
            let decoded = decoded_runs(text, false);
            let spaced = decoded_runs(text, true);
            if !decoded.is_empty() {
                self.scan_as_written(&decoded, is_command);
            }
            if spaced != decoded {
                self.scan_as_written(&spaced, is_command);
            }
        }
    }

    fn scan_as_written(&mut self, text: &str, is_command: bool) {
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
        if self.past_deadline() {
            return;
        }
        let hay = fold(text);
        self.roots(text, &hay);
        self.relative_roots(text, &hay, is_command);
        self.spaced_roots(text, &hay, is_command);
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

    /// Root pass: each spelling of each private root, then of each private
    /// glob's literal directory, where the next character ends it. A path in a
    /// glob's directory asks even when the glob would not match it: the path may
    /// go on past a space the pass cannot see past, and asking is the safe side.
    fn roots(&mut self, text: &str, hay: &str) {
        let forms = std::mem::take(&mut self.root_forms);
        for form in &forms {
            self.form_hits(text, hay, form, true);
        }
        self.root_forms = forms;
    }

    /// Relative spellings of a private root or a private glob's directory,
    /// anywhere in the text. A bare one with no `/` (`notes`, from the root's
    /// parent) counts on its own only in a command, like the bare-name rule;
    /// elsewhere only as the whole string (see `whole`) or with a `/` or `\`
    /// after it, so prose that names it does not ask.
    fn relative_roots(&mut self, text: &str, hay: &str, is_command: bool) {
        let forms = std::mem::take(&mut self.rel_forms);
        let value = whole(hay);
        for form in &forms {
            // A string that is only the bare root names it, in any field.
            let alone = is_command || value == form;
            self.form_hits(text, hay, form, alone);
        }
        self.rel_forms = forms;
    }

    /// Each match of `form` that the next character ends, extended to the end of
    /// the path, unless the allow list covers it. A `form` with no `/` needs a
    /// `/` or `\` after it unless `alone` is set. `hay` is `text` case-folded
    /// as `form` is, with the same byte offsets.
    fn form_hits(&mut self, text: &str, hay: &str, form: &str, alone: bool) {
        let needs_slash = !alone && !form.contains('/');
        for (start, _) in hay.match_indices(form) {
            let end = start + form.len();
            let next = text[end..].chars().next();
            // `\` too: `My Notes\plan.md` written the Windows way.
            let ends = if needs_slash {
                matches!(next, Some('/' | '\\'))
            } else {
                ends_root(&text[end..])
            };
            if !ends {
                continue;
            }
            let stop = text[end..].find(ends_path).map_or(text.len(), |i| end + i);
            let cand = &text[start..stop];
            // A bare relative name has no `/`: it names the root itself.
            let allowed = self
                .abs(cand, false)
                .is_some_and(|p| self.s.allowed_with(self.r, &p));
            if !allowed {
                self.push(cand, HitKind::Root);
            }
        }
    }

    /// Any other spelling of a path into a root with a space, such as
    /// `repo/../My Notes/plan.md`, which the fixed forms miss and the token
    /// pass cuts at the space. Each match of the root's last spaced name after
    /// its parent's name, `..`, `.` or an empty component is read back to each
    /// place its path may start, and the candidate is resolved as a token
    /// would be. Other text before the name can only reach the root through a
    /// symlink, and skipping it keeps prose that names the folder cheap.
    fn spaced_roots(&mut self, text: &str, hay: &str, shell: bool) {
        let names = std::mem::take(&mut self.spaced);
        for Spaced { parent, name } in &names {
            for (start, _) in hay.match_indices(name.as_str()) {
                if self.past_deadline() {
                    break;
                }
                let end = start + name.len();
                let Some(before) = hay[..start].strip_suffix(['/', '\\']) else {
                    continue;
                };
                let via = before.is_empty()
                    || before.ends_with(['/', '\\', '.'])
                    || parent
                        .as_ref()
                        .is_some_and(|p| before.ends_with(p.as_str()));
                if !via || !ends_root(&text[end..]) {
                    continue;
                }
                let stop = text[end..].find(ends_path).map_or(text.len(), |i| end + i);
                for from in path_starts(text, start) {
                    let cand = &text[from..stop];
                    if !self.tokens.insert((cand.to_string(), shell)) {
                        continue;
                    }
                    if self
                        .abs(cand, shell)
                        .is_some_and(|p| self.reaches_private(&p))
                    {
                        self.push(cand, HitKind::Root);
                        break;
                    }
                }
            }
        }
        self.spaced = names;
    }

    /// Inside a private root or a private glob's directory, and not allowed.
    fn reaches_private(&self, p: &Path) -> bool {
        self.s.reaches_with(self.r, p) && !self.s.allowed_with(self.r, p)
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
        if self.past_deadline() || !self.tokens.insert((tok.to_string(), shell)) {
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
mod tests;
