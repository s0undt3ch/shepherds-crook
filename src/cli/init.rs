use std::path::Path;

use crate::project::{self, ItemKind, Record};
use crate::store::{Mode, Store, Touch};
use crate::{Env, Error, config, paths, privacy};

/// Makes `home` crook's, or refuses before any change: a missing or empty
/// directory gets the marker, a home crook made for `slug` is kept, and
/// anything else is the user's, which `wipe` would otherwise empty.
fn adopt_home(home: &Path, slug: &str) -> Result<(), Error> {
    let mut entries = match std::fs::read_dir(home) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return paths::make_home(home, slug),
        Err(e) => return Err(e.into()),
    };
    if entries.next().is_none() {
        return paths::mark_home(home, slug);
    }
    if paths::is_crook_home(home, slug) {
        return Ok(());
    }
    Err(Error::Refused(format!(
        "the project home {} already exists and crook did not make it (no {} naming {slug}); \
         nothing was created: pick another name, or set `home` in config.toml elsewhere",
        home.display(),
        paths::HOME_MARKER
    )))
}

/// Creates the project record, `<home>/<slug>/` and the diary, and records the
/// diary as a notes folder. Makes no `repos/` folder.
pub fn run(env: Env, slug: &str) -> Result<String, Error> {
    paths::valid_slug(slug)?;
    let cfg = config::load(env)?;
    let home = cfg.project_home(env, slug)?;
    let diary = cfg.diary(env, slug)?;
    // The home is the orchestrator's cwd: a private location there would make
    // every ordinary call in it ask. The diary may be private: it is a notes folder.
    let set = privacy::Set::load(&Record::default(), &cfg, env, slug)?;
    if set.holds(&home) {
        return Err(Error::Refused(format!(
            "the project home {} is inside a private location: set `home` in config.toml elsewhere",
            home.display()
        )));
    }
    if paths::project_dir(env, slug)?.join("crook.db").is_file() {
        return Err(Error::Refused(format!("project {slug} already exists")));
    }
    adopt_home(&home, slug)?;
    std::fs::create_dir_all(&diary)?;
    project::init(env, slug)?;
    let diary_text = diary
        .to_str()
        .ok_or_else(|| Error::Refused("the diary path is not valid UTF-8".into()))?;
    let mut store = Store::open(env, slug, Mode::ReadWrite)?;
    store.write(Touch::PROJECT, |tx| {
        project::add(tx, ItemKind::Notes, diary_text).map(|_| ())
    })?;
    Ok(format!(
        "created project {slug}\nhome  {}\ndiary {}",
        home.display(),
        diary.display()
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use super::*;

    fn env_in(dir: &Path, extra: &[(&str, String)]) -> impl Fn(&str) -> Option<String> + use<> {
        let d = dir.display().to_string();
        let mut m: HashMap<String, String> = HashMap::from([
            ("HOME".into(), format!("{d}/h")),
            ("XDG_DATA_HOME".into(), format!("{d}/data")),
            ("XDG_STATE_HOME".into(), format!("{d}/state")),
            ("XDG_CONFIG_HOME".into(), format!("{d}/cfg")),
        ]);
        for (k, v) in extra {
            m.insert((*k).into(), v.clone());
        }
        move |k: &str| m.get(k).cloned()
    }

    fn config(dir: &Path, toml: &str) {
        let f = dir.join("cfg/crook/config.toml");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, toml).unwrap();
    }

    fn notes(env: Env) -> Vec<String> {
        Store::open(env, "demo", Mode::ReadOnly)
            .unwrap()
            .read(project::get)
            .unwrap()
            .notes
    }

    #[test]
    fn init_home() {
        let t = tempfile::tempdir().unwrap();
        let env = env_in(t.path(), &[]);
        run(&env, "demo").unwrap();
        let home = t.path().join("data/crook/demo");
        assert!(home.join("diary").is_dir());
        assert!(!home.join("repos").exists());
        assert_eq!(notes(&env), [home.join("diary").display().to_string()]);
    }

    #[test]
    fn init_home_with_diary_root() {
        let t = tempfile::tempdir().unwrap();
        let vault = t.path().join("vault/projects");
        config(
            t.path(),
            &format!("diary_root = {:?}\n", vault.display().to_string()),
        );
        let env = env_in(t.path(), &[]);
        run(&env, "demo").unwrap();
        let home = t.path().join("data/crook/demo");
        assert!(home.is_dir());
        assert!(!home.join("diary").exists());
        assert!(vault.join("demo").is_dir());
        assert_eq!(notes(&env), [vault.join("demo").display().to_string()]);
    }

    #[test]
    fn init_home_in_a_private_location_is_refused() {
        let t = tempfile::tempdir().unwrap();
        let base = t.path().display().to_string();
        config(
            t.path(),
            &format!("home = \"{base}/vault/crook\"\nprivate = [\"{base}/vault\"]\n"),
        );
        let env = env_in(t.path(), &[]);
        let e = run(&env, "demo").unwrap_err();
        assert!(e.to_string().contains("private location"), "{e}");
        assert!(!t.path().join("vault").exists());
        assert!(!t.path().join("state/crook/demo").exists());
    }

    #[test]
    fn init_refuses_an_existing_project_without_touching_dirs() {
        let t = tempfile::tempdir().unwrap();
        let env = env_in(t.path(), &[]);
        run(&env, "demo").unwrap();
        std::fs::remove_dir_all(t.path().join("data")).unwrap();
        let e = run(&env, "demo").unwrap_err();
        assert!(e.to_string().contains("already exists"), "{e}");
        assert!(!t.path().join("data").exists());
    }

    #[test]
    fn init_marks_the_home_it_makes() {
        let t = tempfile::tempdir().unwrap();
        let env = env_in(t.path(), &[]);
        run(&env, "demo").unwrap();
        assert!(paths::is_crook_home(
            &t.path().join("data/crook/demo"),
            "demo"
        ));
        // An empty directory is made crook's too.
        let empty = t.path().join("data/crook/other");
        std::fs::create_dir_all(&empty).unwrap();
        run(&env, "other").unwrap();
        assert!(paths::is_crook_home(&empty, "other"));
    }

    #[test]
    fn init_adopts_only_a_home_crook_made_for_the_slug() {
        let t = tempfile::tempdir().unwrap();
        let env = env_in(t.path(), &[]);
        run(&env, "demo").unwrap();
        let home = t.path().join("data/crook/demo");
        std::fs::write(home.join("diary/n.md"), "n").unwrap();
        std::fs::remove_dir_all(t.path().join("state")).unwrap();
        run(&env, "demo").unwrap();
        assert!(home.join("diary/n.md").is_file());

        // The same home claimed for another slug, or a directory with no marker.
        for marker in [Some("crook project home: other\n"), None] {
            let _ = std::fs::remove_dir_all(t.path().join("state"));
            match marker {
                Some(m) => std::fs::write(home.join(paths::HOME_MARKER), m).unwrap(),
                None => std::fs::remove_file(home.join(paths::HOME_MARKER)).unwrap(),
            }
            let e = run(&env, "demo").unwrap_err();
            assert!(e.to_string().contains("crook did not make it"), "{e}");
            assert!(!t.path().join("state/crook/demo").exists());
            assert!(home.join("diary/n.md").is_file());
        }
    }

    #[test]
    fn init_refuses_a_bad_home() {
        let t = tempfile::tempdir().unwrap();
        let env = env_in(t.path(), &[]);
        config(t.path(), "home = \"relative/dir\"\n");
        assert!(run(&env, "demo").is_err());
        let inside = format!("home = \"{}/state/crook/x\"\n", t.path().display());
        config(t.path(), &inside);
        let e = run(&env, "demo").unwrap_err();
        assert!(e.to_string().contains("state dir"), "{e}");
    }
}
