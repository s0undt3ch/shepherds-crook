//! Subtracting chunks that are public anyway.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::chunks::normalise_line;
use super::{ChunkHit, Set};

/// Keeps the hits whose chunk is not already public: not in a record link or
/// ticket, not in the edited file or its `old_string`, and not on a line of a
/// tracked file in a project repo outside the private locations.
pub fn subtract(
    hits: Vec<ChunkHit>,
    public: &[String],
    file_content: &str,
    old_string: &str,
    repos: &[PathBuf],
    s: &Set,
) -> Vec<ChunkHit> {
    let mut known: Vec<String> = Vec::new();
    for text in public
        .iter()
        .map(String::as_str)
        .chain([file_content, old_string])
    {
        known.extend(text.lines().map(normalise_line));
    }
    let mut kept: Vec<ChunkHit> = hits
        .into_iter()
        .filter(|h| !covered(&h.chunk, &known))
        .collect();
    for repo in repos {
        if kept.is_empty() {
            break;
        }
        if s.contains(repo) {
            continue;
        }
        let lines = repo_lines(repo, &kept, s);
        kept.retain(|h| !covered(&h.chunk, &lines));
    }
    kept
}

fn covered(chunk: &str, lines: &[String]) -> bool {
    lines.iter().any(|l| l.contains(chunk))
}

/// Whole lines of tracked files that contain any of the chunks, normalised. The
/// chunks go in on stdin, so private text never reaches a file or a command line.
/// A repo that cannot be searched gives no lines.
fn repo_lines(repo: &Path, hits: &[ChunkHit], s: &Set) -> Vec<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo)
        .args(["grep", "-h", "-F", "-f", "-", "--", "."]);
    for rel in private_inside(repo, s) {
        cmd.arg(format!(":(exclude,literal){}", rel.display()));
    }
    let Ok(mut child) = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Vec::new();
    };
    let mut patterns = String::new();
    for h in hits {
        patterns.push_str(&h.chunk);
        patterns.push('\n');
    }
    if let Some(mut stdin) = child.stdin.take() {
        // git may exit before reading everything; its output is judged as it stands.
        let _ = stdin.write_all(patterns.as_bytes());
    }
    let Ok(out) = child.wait_with_output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(normalise_line)
        .collect()
}

/// Private roots inside the repo, relative to it.
fn private_inside(repo: &Path, s: &Set) -> Vec<PathBuf> {
    let mut bases = vec![repo.to_path_buf()];
    if let Ok(real) = std::fs::canonicalize(repo) {
        bases.push(real);
    }
    let mut rels: Vec<PathBuf> = s
        .roots()
        .iter()
        .filter_map(|root| {
            bases
                .iter()
                .find_map(|b| root.strip_prefix(b).ok())
                .filter(|rel| !rel.as_os_str().is_empty())
                .map(Path::to_path_buf)
        })
        .collect();
    rels.sort();
    rels.dedup();
    rels
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use super::*;
    use crate::privacy::fixture::Fx;

    fn hit(chunk: &str) -> ChunkHit {
        ChunkHit {
            chunk: chunk.to_string(),
            file: PathBuf::from("/private/n.md"),
        }
    }

    fn git(repo: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn repo(fx: &Fx, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let r = fx.base.join(name);
        std::fs::create_dir_all(&r).expect("mkdir");
        git(&r, &["init", "-q"]);
        for (p, text) in files {
            fx.write(&r.join(p), text);
        }
        git(&r, &["add", "."]);
        git(&r, &["commit", "-q", "-m", "x"]);
        r
    }

    const NINE: &str = "one two three four five six seven eight nine";
    const C1: &str = "one two three four five six seven eight";
    const C2: &str = "two three four five six seven eight nine";

    #[test]
    fn subtract_public() {
        let fx = Fx::new();
        let public = vec![
            "https://example.com/t/1".to_string(),
            "Fix   the login   flow when the token has expired today".to_string(),
        ];
        let hits = vec![
            hit("Fix the login flow when the token has expired"),
            hit("an unrelated private sentence of exactly eight words"),
        ];
        let got = subtract(hits, &public, "", "", &[], &fx.set(&[]));
        assert_eq!(
            got,
            vec![hit("an unrelated private sentence of exactly eight words")]
        );
    }

    #[test]
    fn subtract_edit_context() {
        let fx = Fx::new();
        let hits = vec![hit(C1), hit("a b c d e f g h"), hit("x y z w v u t s")];
        let got = subtract(
            hits,
            &[],
            &format!("head\n{NINE}\ntail"),
            "a   b c d e f g h and more",
            &[],
            &fx.set(&[]),
        );
        assert_eq!(got, vec![hit("x y z w v u t s")]);
    }

    #[test]
    fn subtract_repo_per_chunk() {
        let fx = Fx::new();
        let r = repo(&fx, "proj", &[("src/lib.rs", &format!("    // {NINE}\n"))]);
        let hits = vec![
            hit(C1),
            hit(C2),
            hit("found nowhere in the repo at all ever"),
        ];
        let got = subtract(hits, &[], "", "", &[r], &fx.set(&[]));
        assert_eq!(got, vec![hit("found nowhere in the repo at all ever")]);
    }

    #[test]
    fn subtract_repo() {
        let fx = Fx::new();
        let secret = "private words that only live in the folder";
        let r = repo(
            &fx,
            "proj",
            &[
                ("src/a.txt", "public line has these eight words in it ok\n"),
                ("pvt/n.md", secret),
            ],
        );
        let set = fx.set(&[&r.join("pvt")]);
        let hits = vec![hit("public line has these eight words in it"), hit(secret)];
        let got = subtract(hits, &[], "", "", std::slice::from_ref(&r), &set);
        assert_eq!(got, vec![hit(secret)], "tracked private folder stays a hit");

        let inside = fx.set(&[&r]);
        let hits = vec![hit("public line has these eight words in it")];
        let got = subtract(hits.clone(), &[], "", "", &[r], &inside);
        assert_eq!(
            got, hits,
            "a repo inside a private location is not searched"
        );
    }

    #[test]
    fn a_missing_or_non_repo_path_is_ignored() {
        let fx = Fx::new();
        let hits = vec![hit(C1)];
        let got = subtract(
            hits.clone(),
            &[],
            "",
            "",
            &[fx.base.join("nope"), fx.base.clone()],
            &fx.set(&[]),
        );
        assert_eq!(got, hits);
    }
}
