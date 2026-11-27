use super::*;
use crate::files::roster::{self, State};
use crate::hc::fake::{Call, FakeHc};
use crate::testutil::{Buffers, FIXED_DATE, Fixture};

fn new(
    fx: &Fixture,
    hc: &FakeHc,
    bufs: &mut Buffers,
    slug: &str,
    task: Option<&str>,
) -> Result<(), Error> {
    run(&fx.ctx(), hc, &mut bufs.io(), slug, task)
}

fn run_specs(hc: &FakeHc) -> Vec<crate::hc::RunSpec> {
    hc.calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Run(s) => Some(s),
            _ => None,
        })
        .collect()
}

#[test]
fn creates_the_folder_and_roster_with_todays_date_and_opens_it() {
    let fx = Fixture::new();
    let hc = FakeHc::new();
    let mut bufs = Buffers::default();
    new(&fx, &hc, &mut bufs, "demo", None).expect("creates");

    let folder = fx.flocks_root().join(format!("{FIXED_DATE}-demo"));
    assert!(folder.join("briefs").is_dir() && folder.join("results").is_dir());
    let on_disk = roster::load(&folder).expect("roster");
    assert_eq!(on_disk.flock, "demo");
    assert_eq!(on_disk.coordinator.state, State::Active);
    assert!(on_disk.coordinator.started, "open ran and set it");
    assert_eq!(on_disk.coordinator.folder, folder);
    assert!(on_disk.children.is_empty());
    assert_eq!(
        on_disk.coordinator.session_id,
        "00000000-0000-4000-8000-000000000001"
    );
    assert_eq!(run_specs(&hc).len(), 1, "one coordinator launch");
    assert_eq!(bufs.out.text(), "opened flock demo (terminal t1)\n");
}

#[test]
fn the_task_reaches_the_first_prompt_and_without_it_only_the_fixed_text_does() {
    let fx = Fixture::new();
    let hc = FakeHc::new();
    new(
        &fx,
        &hc,
        &mut Buffers::default(),
        "with",
        Some("ship the thing"),
    )
    .expect("creates");
    new(&fx, &hc, &mut Buffers::default(), "without", None).expect("creates");
    let specs = run_specs(&hc);
    let with = specs[0].argv.last().expect("prompt");
    let without = specs[1].argv.last().expect("prompt");
    assert!(with.ends_with("ship the thing"));
    assert!(!without.contains("first task"));
}

#[test]
fn an_existing_slug_closed_ones_included_is_refused_before_anything_is_written() {
    let fx = Fixture::new();
    let hc = FakeHc::new();
    new(&fx, &hc, &mut Buffers::default(), "demo", None).expect("creates");
    let folder = fx.flocks_root().join(format!("{FIXED_DATE}-demo"));
    roster::mark_all_done(&folder).expect("close");
    let before = std::fs::read_to_string(folder.join("roster.json")).expect("read");
    let calls_before = hc.calls().len();

    let err = new(&fx, &hc, &mut Buffers::default(), "demo", None).expect_err("refused");
    assert!(err.to_string().contains("already used"), "{err}");
    assert_eq!(hc.calls().len(), calls_before, "no hc call");
    assert_eq!(
        std::fs::read_to_string(folder.join("roster.json")).expect("read"),
        before
    );
    assert_eq!(std::fs::read_dir(fx.flocks_root()).expect("dir").count(), 1);
}

#[test]
fn an_invalid_slug_is_refused_and_nothing_is_created() {
    let fx = Fixture::new();
    let hc = FakeHc::new();
    for bad in ["Demo", "t7", "a--b", "way-too-long-for-a-slug", "-x"] {
        new(&fx, &hc, &mut Buffers::default(), bad, None).expect_err(bad);
    }
    assert!(!fx.flocks_root().exists());
    assert!(hc.calls().is_empty());
}

#[test]
fn a_failed_open_keeps_the_flock_and_says_how_to_finish() {
    let fx = Fixture::new();
    let hc = FakeHc::new();
    hc.fail("run", 1);
    let mut bufs = Buffers::default();
    new(&fx, &hc, &mut bufs, "demo", None).expect_err("fails");
    assert!(
        bufs.err.text().contains("crook open demo"),
        "{}",
        bufs.err.text()
    );
    let folder = fx.flocks_root().join(format!("{FIXED_DATE}-demo"));
    assert!(!roster::load(&folder).expect("roster").coordinator.started);
    // The retry is `open`, not `new`.
    run_open(&fx, &hc);
}

fn run_open(fx: &Fixture, hc: &FakeHc) {
    super::super::open::run(&fx.ctx(), hc, &mut Buffers::default().io(), "demo", None)
        .expect("open works after the failed new");
}

#[test]
fn the_local_date_from_date_wins_and_junk_falls_back_to_the_clock() {
    use crate::run::Output;
    use crate::run::fake::FakeRunner;
    let fx = Fixture::new();
    // The fixture's runner has no `date`, so the clock answers.
    assert_eq!(today(&fx.ctx()), FIXED_DATE);

    let ctx = fx.ctx();
    let good = FakeRunner::new([Ok(Output::success("2031-01-02\n"))]);
    let with_date = Ctx {
        runner: &good,
        ..ctx
    };
    assert_eq!(today(&with_date), "2031-01-02");

    let ctx = fx.ctx();
    let junk = FakeRunner::new([Ok(Output::success("Thu Nov 27\n"))]);
    let with_junk = Ctx {
        runner: &junk,
        ..ctx
    };
    assert_eq!(today(&with_junk), FIXED_DATE);
}

#[test]
fn utc_dates_are_right_across_leap_years() {
    assert_eq!(utc_date(0), "1970-01-01");
    assert_eq!(utc_date(951_782_400), "2000-02-29");
    assert_eq!(utc_date(1_709_251_199), "2024-02-29");
    assert_eq!(utc_date(1_709_251_200), "2024-03-01");
    assert_eq!(utc_date(1_795_780_800), "2026-11-27");
}
