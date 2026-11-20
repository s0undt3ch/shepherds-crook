//! Subtracting chunks that are public anyway.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Instant;

use super::chunks::normalise_line;
use super::{ChunkHit, Set};

/// Keeps the hits whose chunk is not already public: not in a record link or
/// ticket, not in the edited file or its `old_string`, and not on a line of a
/// tracked file in a project repo outside the private locations.
///
/// The repo searches stop at `deadline`: the search is killed and every hit is
/// kept, since a partial answer must never clear a chunk.
pub fn subtract(
    hits: Vec<ChunkHit>,
    public: &[String],
    file_content: &str,
    old_string: &str,
    repos: &[PathBuf],
    s: &Set,
    deadline: Instant,
) -> Vec<ChunkHit> {
    let original = hits.clone();
    let mut known: Vec<String> = Vec::new();
    for text in public
        .iter()
        .map(String::as_str)
        .chain([file_content, old_string])
    {
        known.extend(text.lines().map(padded));
    }
    let mut kept: Vec<ChunkHit> = hits
        .into_iter()
        .filter(|h| !covered(&h.chunk, &known))
        .collect();
    for repo in repos {
        if kept.is_empty() {
            break;
        }
        if Instant::now() >= deadline {
            return original;
        }
        if s.contains(repo) {
            continue;
        }
        let Some(lines) = repo_lines(repo, &kept, s, deadline) else {
            return original;
        };
        kept.retain(|h| !covered(&h.chunk, &lines));
    }
    kept
}

/// A line as `normalise_line` makes it, padded with a space each side so a
/// chunk matches whole words only.
fn padded(line: &str) -> String {
    format!(" {} ", normalise_line(line))
}

fn covered(chunk: &str, lines: &[String]) -> bool {
    let needle = format!(" {chunk} ");
    lines.iter().any(|l| l.contains(&needle))
}

/// Whole lines of tracked files that contain any of the chunks, padded. The
/// chunks go in on stdin, so private text never reaches a file or a command line.
/// Lines from a file that is private by any rule (glob included) are dropped.
/// A repo that cannot be searched gives no lines. `None` means the search did
/// not finish by `deadline` and was killed.
fn repo_lines(repo: &Path, hits: &[ChunkHit], s: &Set, deadline: Instant) -> Option<Vec<String>> {
    let Ok(mut child) = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["grep", "-F", "--null", "-f", "-", "--", "."])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Some(Vec::new());
    };
    let mut patterns = String::new();
    for h in hits {
        patterns.push_str(&h.chunk);
        patterns.push('\n');
    }
    // Both pipes are served from threads, so neither a full stdin nor a full
    // stdout can stall the wait below. git may exit before reading everything;
    // its output is judged as it stands.
    if let Some(mut stdin) = child.stdin.take() {
        std::thread::spawn(move || {
            let _ = stdin.write_all(patterns.as_bytes());
        });
    }
    let (tx, rx) = mpsc::channel();
    if let Some(mut stdout) = child.stdout.take() {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = stdout.read_to_end(&mut buf);
            let _ = tx.send(buf);
        });
    }
    let wait = deadline.saturating_duration_since(Instant::now());
    let Ok(stdout) = rx.recv_timeout(wait) else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    let _ = child.wait();
    let mut lines = Vec::new();
    // `--null` puts a NUL between the path and the line and quotes nothing, so a
    // path may hold a newline: a row without a NUL is the start of a path.
    let mut row: Vec<u8> = Vec::new();
    for part in stdout.split(|&b| b == b'\n') {
        if Instant::now() >= deadline {
            return None;
        }
        row.extend_from_slice(part);
        let Some(nul) = row.iter().position(|&b| b == 0) else {
            row.push(b'\n');
            continue;
        };
        let path = repo.join(OsStr::from_bytes(&row[..nul]));
        if !s.contains(&path) {
            lines.push(padded(&String::from_utf8_lossy(&row[nul + 1..])));
        }
        row.clear();
    }
    Some(lines)
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use std::time::Duration;

    use super::*;
    use crate::privacy::fixture::Fx;

    /// `subtract` with a deadline far enough away to never matter.
    fn subtract(
        hits: Vec<ChunkHit>,
        public: &[String],
        file_content: &str,
        old_string: &str,
        repos: &[PathBuf],
        s: &Set,
    ) -> Vec<ChunkHit> {
        let far = Instant::now() + Duration::from_secs(60);
        super::subtract(hits, public, file_content, old_string, repos, s, far)
    }

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

    #[test]
    fn a_glob_private_file_tracked_in_a_repo_stays_a_hit() {
        let fx = Fx::new();
        let secret = "private words that only live in the folder";
        let r = repo(
            &fx,
            "proj",
            &[
                ("pvt/x.md", secret),
                ("pvt/y.txt", "other lines in the public file here ok\n"),
            ],
        );
        let set = fx.set(&[&r.join("pvt/*.md")]);
        let hits = vec![hit(secret), hit("other lines in the public file here")];
        let got = subtract(hits, &[], "", "", &[r], &set);
        assert_eq!(got, vec![hit(secret)]);
    }

    #[test]
    fn a_chunk_starting_mid_word_is_not_covered() {
        let fx = Fx::new();
        let public = vec!["bathe cat sat on the mat and then left".to_string()];
        let mid = hit("he cat sat on the mat and then");
        let whole = hit("cat sat on the mat and then left");
        let got = subtract(vec![mid.clone(), whole], &public, "", "", &[], &fx.set(&[]));
        assert_eq!(got, vec![mid]);
        let r = repo(
            &fx,
            "proj",
            &[("a.txt", "bathe cat sat on the mat and then left\n")],
        );
        let got = subtract(
            vec![hit("he cat sat on the mat and then")],
            &[],
            "",
            "",
            &[r],
            &fx.set(&[]),
        );
        assert_eq!(got.len(), 1);
    }

    #[test]
    fn a_newline_in_a_tracked_path_does_not_misattribute_it() {
        let fx = Fx::new();
        let secret = "private words that only live in the folder";
        let r = repo(&fx, "proj", &[("pvt/x\nfoo.md", secret)]);
        let set = fx.set(&[&r.join("pvt/*.md")]);
        let got = subtract(vec![hit(secret)], &[], "", "", &[r], &set);
        assert_eq!(got, vec![hit(secret)]);
    }

    #[test]
    fn deadline_keeps_every_hit() {
        let fx = Fx::new();
        let secret = "alpha beta gamma delta epsilon zeta eta theta";
        let r = repo(&fx, "proj", &[("a.txt", &format!("{secret}\n"))]);
        let hits = vec![hit(secret)];
        let set = fx.set(&[]);
        let now = Instant::now();
        let far = now + Duration::from_secs(60);
        let sub = |d| super::subtract(hits.clone(), &[], "", "", std::slice::from_ref(&r), &set, d);
        assert!(sub(far).is_empty(), "subtracted when there is time");
        assert_eq!(sub(now), hits, "kept when the time is up");
    }
}
