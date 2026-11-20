//! The verbatim index of private files: every 8-word shingle of every private
//! file, hashed, in one file under the project's state directory.
//!
//! Layout of `index/index.bin`, little-endian: the magic `CRKI`, the format
//! version, the file count and the record count (`u32` each); the manifest, one
//! entry per file (`u32` id, `u64` mtime in nanoseconds, `u32` path length, the
//! path bytes); then the records, sorted, each a `u64` hash and a `u32` file id.
//! One file means one `rename` replaces it, so a concurrent reader never sees a
//! torn pair. A length, count or id that does not add up means corrupt, and
//! `refresh` rebuilds from the private files.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use super::Set;
use super::chunks::{fnv1a, shingles};
use crate::Error;

const MAGIC: &[u8; 4] = b"CRKI";
const VERSION: u32 = 1;
const HEADER: usize = 16;
const MAX_FILE: u64 = 1 << 20;
const BINARY_PROBE: usize = 8 * 1024;
/// More than this many files and the index covers the first of them in path order.
pub const MAX_FILES: usize = 20_000;

/// A chunk of text that is also in a private file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkHit {
    pub chunk: String,
    pub file: PathBuf,
}

struct Entry {
    path: PathBuf,
    mtime: u64,
}

#[derive(Default)]
pub struct Index {
    files: Vec<Entry>,
    /// Sorted by hash, then file id.
    records: Vec<(u64, u32)>,
    capped: Option<PathBuf>,
}

impl Index {
    /// Brings `<dir>/index/index.bin` up to date with the private files and
    /// returns it. A file whose mtime is unchanged keeps its old records; a
    /// missing or corrupt index is rebuilt in full.
    pub fn refresh(dir: &Path, s: &Set) -> Result<Index, Error> {
        Index::refresh_capped(dir, s, MAX_FILES)
    }

    fn refresh_capped(dir: &Path, s: &Set, cap: usize) -> Result<Index, Error> {
        let file = dir.join("index/index.bin");
        let old = Index::load(&file).unwrap_or_default();
        let (found, capped) = scan(dir, s, cap);

        let mut old_hashes: Vec<Vec<u64>> = vec![Vec::new(); old.files.len()];
        for &(h, id) in &old.records {
            if let Some(v) = old_hashes.get_mut(id as usize) {
                v.push(h);
            }
        }
        let old_by_path: HashMap<&Path, usize> = old
            .files
            .iter()
            .enumerate()
            .map(|(i, e)| (e.path.as_path(), i))
            .collect();

        let mut files = Vec::new();
        let mut records = Vec::new();
        for (path, mtime) in found {
            let id = u32::try_from(files.len()).unwrap_or(u32::MAX);
            let reused = old_by_path
                .get(path.as_path())
                .filter(|&&i| old.files[i].mtime == mtime)
                .map(|&i| old_hashes[i].as_slice());
            match reused {
                Some(hashes) => records.extend(hashes.iter().map(|&h| (h, id))),
                None => {
                    let Some(text) = read_text(&path) else {
                        continue;
                    };
                    let unique: BTreeSet<u64> = shingles(&text).iter().map(|c| fnv1a(c)).collect();
                    records.extend(unique.into_iter().map(|h| (h, id)));
                }
            }
            files.push(Entry { path, mtime });
        }
        records.sort_unstable();
        let idx = Index {
            files,
            records,
            capped,
        };
        idx.store(&file)?;
        Ok(idx)
    }

    /// Every 8-word chunk of `text` that is in a private file, once per file.
    pub fn lookup(&self, text: &str) -> Vec<ChunkHit> {
        let mut seen: HashSet<(u64, u32)> = HashSet::new();
        let mut out = Vec::new();
        for chunk in shingles(text) {
            let h = fnv1a(&chunk);
            let start = self.records.partition_point(|r| r.0 < h);
            for &(rh, id) in self.records[start..].iter().take_while(|r| r.0 == h) {
                if let Some(e) = self.files.get(id as usize)
                    && seen.insert((rh, id))
                {
                    out.push(ChunkHit {
                        chunk: chunk.clone(),
                        file: e.path.clone(),
                    });
                }
            }
        }
        out
    }

    /// The private root that held files beyond the cap, when there were any.
    /// Those files are not indexed.
    pub fn capped(&self) -> Option<&Path> {
        self.capped.as_deref()
    }

    /// `None` for a missing file and for any header, length, count or id that
    /// does not add up.
    fn load(file: &Path) -> Option<Index> {
        let raw = fs::read(file).ok()?;
        if raw.get(..4)? != MAGIC || u32_at(&raw, 4)? != VERSION {
            return None;
        }
        let nfiles = u32_at(&raw, 8)? as usize;
        let nrecs = u32_at(&raw, 12)? as usize;
        let mut at = HEADER;
        let mut files = Vec::new();
        for want in 0..nfiles {
            if u32_at(&raw, at)? as usize != want {
                return None;
            }
            let mtime = u64_at(&raw, at + 4)?;
            let len = u32_at(&raw, at + 12)? as usize;
            let start = at + 16;
            let end = start.checked_add(len)?;
            let path = PathBuf::from(OsStr::from_bytes(raw.get(start..end)?));
            files.push(Entry { path, mtime });
            at = end;
        }
        if raw.len() != at.checked_add(nrecs.checked_mul(12)?)? {
            return None;
        }
        let mut records = Vec::with_capacity(nrecs);
        for i in 0..nrecs {
            let o = at + i * 12;
            let id = u32_at(&raw, o + 8)?;
            if id as usize >= nfiles {
                return None;
            }
            records.push((u64_at(&raw, o)?, id));
        }
        Some(Index {
            files,
            records,
            capped: None,
        })
    }

    /// Writes a temp file beside the index, then renames it over the index.
    fn store(&self, file: &Path) -> Result<(), Error> {
        let mut buf = Vec::with_capacity(HEADER + self.records.len() * 12);
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&VERSION.to_le_bytes());
        buf.extend_from_slice(
            &u32::try_from(self.files.len())
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        buf.extend_from_slice(
            &u32::try_from(self.records.len())
                .unwrap_or(u32::MAX)
                .to_le_bytes(),
        );
        for (id, e) in self.files.iter().enumerate() {
            let path = e.path.as_os_str().as_bytes();
            buf.extend_from_slice(&u32::try_from(id).unwrap_or(u32::MAX).to_le_bytes());
            buf.extend_from_slice(&e.mtime.to_le_bytes());
            buf.extend_from_slice(&u32::try_from(path.len()).unwrap_or(u32::MAX).to_le_bytes());
            buf.extend_from_slice(path);
        }
        for &(h, id) in &self.records {
            buf.extend_from_slice(&h.to_le_bytes());
            buf.extend_from_slice(&id.to_le_bytes());
        }
        let parent = file.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(parent)?;
        let tmp = parent.join(format!("index.bin.tmp-{}", std::process::id()));
        let done = fs::write(&tmp, &buf).and_then(|()| fs::rename(&tmp, file));
        if done.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        Ok(done?)
    }
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        b.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(
        b.get(at..at.checked_add(8)?)?.try_into().ok()?,
    ))
}

/// The regular files under every private root, as `(path, mtime)` in path order,
/// cut at `cap`. The state directory is skipped: it holds this index.
fn scan(state: &Path, s: &Set, cap: usize) -> (Vec<(PathBuf, u64)>, Option<PathBuf>) {
    let state = fs::canonicalize(state).unwrap_or_else(|_| state.to_path_buf());
    let roots: BTreeSet<PathBuf> = s
        .roots()
        .iter()
        .filter_map(|r| fs::canonicalize(r).ok())
        .collect();
    let mut found: HashMap<PathBuf, (u64, PathBuf)> = HashMap::new();
    for root in &roots {
        let mut files = Vec::new();
        walk(root, &mut files);
        for (path, mtime) in files {
            if !path.starts_with(&state) {
                found.entry(path).or_insert((mtime, root.clone()));
            }
        }
    }
    let glob_dirs: BTreeSet<PathBuf> = s
        .glob_dirs()
        .iter()
        .filter_map(|d| fs::canonicalize(d).ok())
        .collect();
    for dir in &glob_dirs {
        let mut files = Vec::new();
        walk(dir, &mut files);
        for (path, mtime) in files {
            if !path.starts_with(&state) && s.contains(&path) {
                found.entry(path).or_insert((mtime, dir.clone()));
            }
        }
    }
    let mut all: Vec<(PathBuf, (u64, PathBuf))> = found.into_iter().collect();
    all.sort_by(|a, b| a.0.cmp(&b.0));
    let capped = all.get(cap).map(|(_, (_, root))| root.clone());
    all.truncate(cap);
    (all.into_iter().map(|(p, (m, _))| (p, m)).collect(), capped)
}

/// Collects regular files, skipping `.git`, symlinked directories and files over
/// the size cap. A symlink to a regular file counts as that file.
fn walk(path: &Path, out: &mut Vec<(PathBuf, u64)>) {
    let Ok(mut meta) = fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_symlink() {
        match fs::metadata(path) {
            Ok(target) if target.is_file() => meta = target,
            _ => return,
        }
    }
    if meta.is_file() {
        if meta.len() <= MAX_FILE {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
            out.push((path.to_path_buf(), mtime));
        }
    } else if meta.is_dir() {
        let Ok(entries) = fs::read_dir(path) else {
            return;
        };
        for e in entries.flatten() {
            if e.file_name() != ".git" {
                walk(&e.path(), out);
            }
        }
    }
}

/// The file as text, or `None` when it is unreadable or binary (a NUL in its first 8 KiB).
fn read_text(path: &Path) -> Option<String> {
    let raw = fs::read(path).ok()?;
    if raw[..raw.len().min(BINARY_PROBE)].contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(&raw).into_owned())
}

#[cfg(test)]
mod tests {
    use std::fs::{File, FileTimes};
    use std::time::{Duration, SystemTime};

    use super::*;
    use crate::privacy::fixture::Fx;

    const A: &str = "alpha bravo charlie delta echo foxtrot golf hotel india";
    const B: &str = "one two three four five six seven eight nine";

    fn index_path(fx: &Fx) -> PathBuf {
        fx.project_dir().join("index/index.bin")
    }

    fn touch(path: &Path, secs: u64) {
        let f = File::options().write(true).open(path).expect("open");
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
        f.set_times(FileTimes::new().set_modified(t))
            .expect("times");
    }

    #[test]
    fn lookup_names_file() {
        let fx = Fx::new();
        let note = fx.notes.join("a.md");
        fx.write(&note, A);
        let idx = Index::refresh(&fx.project_dir(), &fx.set(&[])).expect("refresh");
        let hits = idx.lookup(&format!("quoted: {A} end"));
        assert!(hits.iter().all(|h| h.file == note), "{hits:?}");
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert!(
            idx.lookup("nothing here at all matches any of the notes ok")
                .is_empty()
        );
    }

    #[test]
    fn refresh_by_mtime() {
        let fx = Fx::new();
        let (a, b) = (fx.notes.join("a.md"), fx.notes.join("b.md"));
        fx.write(&a, A);
        fx.write(&b, B);
        touch(&a, 1_000);
        touch(&b, 1_000);
        let (dir, set) = (fx.project_dir(), fx.set(&[]));
        let idx = Index::refresh(&dir, &set).expect("refresh");
        assert_eq!(idx.lookup(A).len(), 2);
        assert_eq!(idx.lookup(B).len(), 2);

        let c = "kilo lima mike november oscar papa quebec romeo sierra";
        fx.write(&a, c);
        touch(&a, 2_000);
        std::fs::remove_file(&b).expect("rm");
        let idx = Index::refresh(&dir, &set).expect("refresh");
        assert!(idx.lookup(A).is_empty());
        assert_eq!(idx.lookup(c).len(), 2);
        assert!(idx.lookup(B).is_empty(), "deleted file drops out");
    }

    #[test]
    fn unchanged_mtime_is_not_re_read() {
        let fx = Fx::new();
        let a = fx.notes.join("a.md");
        fx.write(&a, A);
        touch(&a, 1_000);
        let (dir, set) = (fx.project_dir(), fx.set(&[]));
        Index::refresh(&dir, &set).expect("refresh");
        let c = "kilo lima mike november oscar papa quebec romeo sierra";
        fx.write(&a, c);
        touch(&a, 1_000);
        let idx = Index::refresh(&dir, &set).expect("refresh");
        assert_eq!(idx.lookup(A).len(), 2, "kept from the old index");
        assert!(idx.lookup(c).is_empty());
    }

    #[test]
    fn write_is_atomic_and_leaves_no_lock() {
        let fx = Fx::new();
        fx.write(&fx.notes.join("a.md"), A);
        let dir = fx.project_dir();
        Index::refresh(&dir, &fx.set(&[])).expect("refresh");
        Index::refresh(&dir, &fx.set(&[])).expect("refresh");
        let names: Vec<String> = std::fs::read_dir(dir.join("index"))
            .expect("read_dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["index.bin".to_string()]);
        let raw = std::fs::read(index_path(&fx)).expect("read");
        assert_eq!(&raw[..4], b"CRKI");
    }

    #[test]
    fn corrupt_index_rebuilds_without_error() {
        let fx = Fx::new();
        fx.write(&fx.notes.join("a.md"), A);
        let (dir, set) = (fx.project_dir(), fx.set(&[]));
        Index::refresh(&dir, &set).expect("refresh");
        let path = index_path(&fx);
        let raw = std::fs::read(&path).expect("read");
        for bad in [
            raw[..raw.len() - 5].to_vec(),
            raw[..6].to_vec(),
            Vec::new(),
            b"garbage that is not an index at all".to_vec(),
            [raw.clone(), vec![0; 3]].concat(),
        ] {
            std::fs::write(&path, bad).expect("write");
            let idx = Index::refresh(&dir, &set).expect("rebuilds");
            assert_eq!(idx.lookup(A).len(), 2);
        }
        std::fs::remove_file(&path).expect("rm");
        assert_eq!(
            Index::refresh(&dir, &set).expect("missing").lookup(A).len(),
            2
        );
    }

    #[test]
    fn index_scope() {
        let fx = Fx::new();
        fx.write(&fx.notes.join(".git/config"), A);
        fx.write(
            &fx.notes.join("big.txt"),
            &format!("{B}\n{}", "x".repeat(1 << 20)),
        );
        fx.write(&fx.notes.join("bin.dat"), &format!("{A}\0 and more"));
        fx.write(&fx.notes.join("sub/ok.md"), "p1 p2 p3 p4 p5 p6 p7 p8 p9");
        let idx = Index::refresh(&fx.project_dir(), &fx.set(&[])).expect("refresh");
        assert!(idx.lookup(A).is_empty(), ".git and binary skipped");
        assert!(idx.lookup(B).is_empty(), "over 1 MiB skipped");
        assert_eq!(idx.lookup("p1 p2 p3 p4 p5 p6 p7 p8 p9").len(), 2);
    }

    #[test]
    fn the_state_dir_is_never_indexed() {
        let fx = Fx::new();
        let dir = fx.project_dir();
        fx.write(&dir.join("notes.txt"), A);
        let idx = Index::refresh(&dir, &fx.set(&[])).expect("refresh");
        assert!(idx.lookup(A).is_empty());
    }

    #[test]
    fn a_private_file_root_and_a_second_root_are_indexed_once() {
        let fx = Fx::new();
        let single = fx.base.join("solo.txt");
        fx.write(&single, B);
        let idx = Index::refresh(&fx.project_dir(), &fx.set(&[&single, &fx.notes])).expect("r");
        let hits = idx.lookup(B);
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert!(hits.iter().all(|h| h.file == single));
    }

    #[test]
    fn over_the_cap_indexes_the_first_files_in_path_order() {
        let fx = Fx::new();
        for (name, text) in [
            ("a.md", A),
            ("b.md", B),
            ("c.md", "p1 p2 p3 p4 p5 p6 p7 p8 p9"),
        ] {
            fx.write(&fx.notes.join(name), text);
        }
        let idx = Index::refresh_capped(&fx.project_dir(), &fx.set(&[]), 2).expect("refresh");
        assert_eq!(idx.capped(), Some(fx.notes.as_path()));
        assert_eq!(idx.lookup(A).len(), 2);
        assert_eq!(idx.lookup(B).len(), 2);
        assert!(idx.lookup("p1 p2 p3 p4 p5 p6 p7 p8 p9").is_empty());
        let all = Index::refresh(&fx.project_dir(), &fx.set(&[])).expect("refresh");
        assert!(all.capped().is_none());
    }

    #[test]
    fn glob_private_files_are_indexed() {
        let fx = Fx::new();
        let g = fx.base.join("g");
        fx.write(&g.join("a.md"), A);
        fx.write(&g.join("b.txt"), B);
        let set = fx.set(&[&g.join("*.md")]);
        let idx = Index::refresh(&fx.project_dir(), &set).expect("refresh");
        let hits = idx.lookup(A);
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert!(hits.iter().all(|h| h.file == g.join("a.md")));
        assert!(idx.lookup(B).is_empty(), "not matched by the glob");
    }

    #[test]
    fn a_symlinked_file_is_indexed_but_a_symlinked_dir_is_not_followed() {
        let fx = Fx::new();
        let (out, outdir) = (fx.base.join("outside.txt"), fx.base.join("outdir"));
        fx.write(&out, B);
        fx.write(&outdir.join("x.md"), A);
        std::os::unix::fs::symlink(&out, fx.notes.join("link.md")).expect("symlink");
        std::os::unix::fs::symlink(&outdir, fx.notes.join("dirlink")).expect("symlink");
        let idx = Index::refresh(&fx.project_dir(), &fx.set(&[])).expect("refresh");
        let hits = idx.lookup(B);
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert!(hits.iter().all(|h| h.file == fx.notes.join("link.md")));
        assert!(idx.lookup(A).is_empty());
    }
}
