//! Shared test setup: a temp home, a private notes folder and a `Set` over them.

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::Set;
use crate::config::Config;
use crate::paths;
use crate::project::Record;

pub struct Fx {
    _tmp: TempDir,
    pub base: PathBuf,
    pub notes: PathBuf,
}

impl Fx {
    pub fn new() -> Fx {
        let tmp = TempDir::new().expect("tempdir");
        let base = std::fs::canonicalize(tmp.path()).expect("canonicalize");
        let notes = base.join("notes");
        std::fs::create_dir_all(&notes).expect("notes dir");
        Fx {
            _tmp: tmp,
            base,
            notes,
        }
    }

    pub fn env(&self) -> impl Fn(&str) -> Option<String> + use<> {
        let home = self.base.join("home").display().to_string();
        let state = self.base.join("state").display().to_string();
        move |k: &str| match k {
            "HOME" => Some(home.clone()),
            "XDG_STATE_HOME" => Some(state.clone()),
            _ => None,
        }
    }

    pub fn set(&self, private: &[&Path]) -> Set {
        let rec = Record {
            private: private.iter().map(|p| p.display().to_string()).collect(),
            notes: vec![self.notes.display().to_string()],
            ..Record::default()
        };
        Set::load(&rec, &Config::default(), &self.env(), "demo")
    }

    /// The project's state directory, where `index/index.bin` lives.
    pub fn project_dir(&self) -> PathBuf {
        paths::project_dir(&self.env(), "demo").expect("project dir")
    }

    pub fn write(&self, path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, text).expect("write");
    }
}
