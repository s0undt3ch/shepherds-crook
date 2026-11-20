//! The private set: roots and globs the user declared, and the allow list.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use globset::{GlobBuilder, GlobMatcher};

use crate::config::Config;
use crate::project::Record;
use crate::{Env, paths};

const GLOB_META: [char; 4] = ['*', '?', '[', '{'];

/// Where private material lives, and the known false positives inside it.
pub struct Set {
    home: PathBuf,
    /// Every spelling of the home directory, as a path.
    home_variants: Vec<PathBuf>,
    /// Private directories, each in its lexical and canonical form.
    roots: Vec<PathBuf>,
    /// Private globs, each written as given and with its literal prefix canonicalised.
    globs: Vec<GlobMatcher>,
    /// Every way a private root can be written inside a string.
    forms: Vec<String>,
    allow_roots: Vec<PathBuf>,
    allow_globs: Vec<GlobMatcher>,
}

impl Set {
    /// Builds the set from the project's notes and `private` lists, the user-wide
    /// `private` list and the project's data directory, which holds the verbatim
    /// index of private files. `allow` entries come from the record and the config.
    pub fn load(rec: &Record, cfg: &Config, env: Env, slug: &str) -> Set {
        let r = Resolver::default();
        let home = paths::home(env);
        let mut home_variants = r.variants(&home);
        home_variants.dedup();
        let mut s = Set {
            home,
            home_variants,
            roots: Vec::new(),
            globs: Vec::new(),
            forms: Vec::new(),
            allow_roots: Vec::new(),
            allow_globs: Vec::new(),
        };
        let private = rec.notes.iter().chain(&rec.private).chain(&cfg.private);
        for entry in private {
            s.add_private(&r, entry);
        }
        if let Ok(dir) = paths::project_dir(env, slug) {
            s.add_private(&r, &dir.display().to_string());
        }
        for entry in rec.allow.iter().chain(&cfg.allow) {
            s.add_allow(&r, entry);
        }
        s.forms.sort();
        s.forms.dedup();
        s
    }

    pub fn contains(&self, abs: &Path) -> bool {
        self.contains_with(&Resolver::default(), abs)
    }

    pub fn allowed(&self, abs: &Path) -> bool {
        self.allowed_with(&Resolver::default(), abs)
    }

    pub(crate) fn forms(&self) -> &[String] {
        &self.forms
    }

    pub(crate) fn contains_with(&self, r: &Resolver, abs: &Path) -> bool {
        r.variants(abs).iter().any(|v| {
            self.roots.iter().any(|root| v.starts_with(root))
                || v.ancestors()
                    .any(|a| self.globs.iter().any(|g| g.is_match(a)))
        })
    }

    /// Judged on the resolved path alone: a lexical spelling inside an allow
    /// root must not allow a symlink that leads out of it. A missing path
    /// resolves to its existing prefix plus the lexical tail.
    pub(crate) fn allowed_with(&self, r: &Resolver, abs: &Path) -> bool {
        let v = r.resolve(abs);
        self.allow_roots.iter().any(|root| v.starts_with(root))
            || self.allow_globs.iter().any(|g| g.is_match(&v))
    }

    /// A glob starting with this literal prefix may expand into a private root:
    /// the prefix is inside a root, or a root's path starts with it (`~/no*`
    /// against `~/notes`). The directory part is canonicalised, the partial
    /// segment kept as written.
    pub(crate) fn glob_reaches_private(&self, r: &Resolver, full: &str) -> bool {
        let end = full.find(GLOB_META).unwrap_or(full.len());
        let dir = literal_dir(full);
        let partial = full.get(dir.len()..end).unwrap_or_default();
        if dir.is_empty() {
            return false;
        }
        r.variants(Path::new(&dir)).iter().any(|v| {
            let pre = format!(
                "{}{}",
                v.display().to_string().trim_end_matches('/'),
                partial
            );
            self.contains_with(r, v)
                || self
                    .roots
                    .iter()
                    .any(|root| root.display().to_string().starts_with(&pre))
        })
    }

    /// `~`, `$HOME` and `${HOME}` for the home directory, as a prefix only.
    pub(crate) fn expand_home(&self, text: &str) -> Option<PathBuf> {
        for pre in ["${HOME}", "$HOME", "~"] {
            if let Some(rest) = text.strip_prefix(pre) {
                if rest.is_empty() {
                    return Some(self.home.clone());
                }
                if let Some(rel) = rest.strip_prefix('/') {
                    return Some(self.home.join(rel));
                }
            }
        }
        None
    }

    /// Absolute path for an entry as the user wrote it; `None` for a relative one,
    /// which has nothing to resolve against.
    fn absolute(&self, entry: &str) -> Option<String> {
        if let Some(p) = self.expand_home(entry) {
            return Some(p.display().to_string());
        }
        entry.starts_with('/').then(|| entry.to_string())
    }

    fn add_private(&mut self, r: &Resolver, entry: &str) {
        let Some(abs) = self.absolute(entry) else {
            return;
        };
        if abs.contains(GLOB_META) {
            match r.glob(&abs) {
                Some(mut g) => self.globs.append(&mut g),
                // A broken glob still protects its directory, which errs towards asking.
                None => self.add_root(r, entry, &literal_dir(&abs)),
            }
            // A directory whose name has a glob character is a root as well.
        }
        self.add_root(r, entry, &abs);
    }

    fn add_allow(&mut self, r: &Resolver, entry: &str) {
        let Some(abs) = self.absolute(entry) else {
            return;
        };
        if abs.contains(GLOB_META) {
            if let Some(mut g) = r.glob(&abs) {
                self.allow_globs.append(&mut g);
            }
        } else {
            self.allow_roots.extend(r.variants(Path::new(&abs)));
        }
    }

    fn add_root(&mut self, r: &Resolver, raw: &str, abs: &str) {
        let variants = r.variants(Path::new(abs));
        let mut forms = vec![raw.to_string()];
        for v in &variants {
            forms.push(v.display().to_string());
            for h in &self.home_variants {
                if let Ok(rel) = v.strip_prefix(h) {
                    let rel = rel.display().to_string();
                    for pre in ["~", "$HOME", "${HOME}"] {
                        forms.push(if rel.is_empty() {
                            pre.to_string()
                        } else {
                            format!("{pre}/{rel}")
                        });
                    }
                }
            }
            // macOS: `/tmp`, `/var` and `/etc` are links into `/private`.
            if let Ok(rest) = v.strip_prefix("/private") {
                forms.push(Path::new("/").join(rest).display().to_string());
            }
        }
        self.forms
            .extend(forms.into_iter().filter(|f| !f.is_empty() && f != "/"));
        self.roots.extend(variants);
    }
}

/// The directory part of a glob before its first metacharacter.
pub(crate) fn literal_dir(pattern: &str) -> String {
    let end = pattern.find(GLOB_META).unwrap_or(pattern.len());
    match pattern[..end].rfind('/') {
        Some(0) => "/".to_string(),
        Some(i) => pattern[..i].to_string(),
        None => String::new(),
    }
}

fn escape_glob(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '{' | '}' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Absolute path with `.` and `..` folded and no filesystem access.
pub(crate) fn lexical(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::RootDir => out.push("/"),
            Component::Normal(n) => out.push(n),
            Component::ParentDir => {
                if out != Path::new("/") {
                    out.pop();
                }
            }
            Component::CurDir | Component::Prefix(_) => {}
        }
    }
    out
}

/// Canonicalises paths by their longest existing prefix. It caches each prefix,
/// so a large input with many path-like tokens costs a few system calls, not
/// one per token.
#[derive(Default)]
pub(crate) struct Resolver {
    cache: RefCell<HashMap<PathBuf, Option<PathBuf>>>,
}

impl Resolver {
    fn canon(&self, p: &Path) -> Option<PathBuf> {
        if let Some(hit) = self.cache.borrow().get(p) {
            return hit.clone();
        }
        let got = std::fs::canonicalize(p).ok();
        self.cache.borrow_mut().insert(p.to_path_buf(), got.clone());
        got
    }

    /// The kernel's view of the longest existing prefix (`..` through symlinks
    /// included), then any `.` and `..` left in the missing tail folded lexically.
    /// Walks down from the root so each shared directory is looked up once; a
    /// component that is missing ends the walk, because nothing below it exists.
    pub(crate) fn resolve(&self, p: &Path) -> PathBuf {
        if !p.is_absolute() {
            return lexical(p);
        }
        let comps: Vec<Component> = p.components().collect();
        let mut prefix = PathBuf::new();
        let mut best: Option<(usize, PathBuf)> = None;
        for (n, c) in comps.iter().enumerate() {
            prefix.push(c);
            match self.canon(&prefix) {
                Some(real) => best = Some((n + 1, real)),
                None if matches!(c, Component::ParentDir) => continue,
                None => break,
            }
        }
        let Some((n, mut base)) = best else {
            return lexical(p);
        };
        for c in &comps[n..] {
            match c {
                Component::Normal(x) => base.push(x),
                Component::ParentDir => {
                    base.pop();
                }
                _ => {}
            }
        }
        base
    }

    /// The path as written (cleaned) and as the filesystem sees it.
    pub(crate) fn variants(&self, p: &Path) -> Vec<PathBuf> {
        let mut v = vec![lexical(p)];
        let real = self.resolve(p);
        if real != v[0] {
            v.push(real);
        }
        v
    }

    /// Matchers for a glob as written and with its literal directory canonicalised.
    fn glob(&self, pattern: &str) -> Option<Vec<GlobMatcher>> {
        let dir = literal_dir(pattern);
        let rest = pattern
            .get(dir.len()..)
            .map(|r| r.trim_start_matches('/'))
            .unwrap_or_default();
        let mut patterns = Vec::new();
        for d in self.variants(Path::new(&dir)) {
            let d = escape_glob(&d.display().to_string());
            patterns.push(format!("{}/{}", d.trim_end_matches('/'), rest));
        }
        patterns.dedup();
        let mut out = Vec::new();
        for p in patterns {
            let g = GlobBuilder::new(&p).literal_separator(true).build().ok()?;
            out.push(g.compile_matcher());
        }
        Some(out)
    }
}
