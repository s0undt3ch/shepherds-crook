use super::*;

fn record() -> Record {
    Record {
        what: "A thing".into(),
        why: "Because".into(),
        status_text: "Halfway".into(),
        repos: vec!["/r/main".into()],
        links: vec!["https://example.test/pr/1".into()],
        tickets: vec!["T-42".into()],
        private: vec!["/n/notes".into()],
        ..Record::default()
    }
}

fn kid(i: usize) -> ChildLine {
    ChildLine {
        id: format!("c{i}"),
        repo: format!("/r/{i}"),
        goal: format!("goal {i}"),
        worktrees: Vec::new(),
    }
}

#[test]
fn card_contents() {
    let me = SelfPart {
        brief: "Do the thing".into(),
        current_task: Some("task one".into()),
        question: Some("which way?".into()),
    };
    let orch = render(&record(), &[kid(1), kid(2)], None);
    for want in [
        "What: A thing",
        "Why: Because",
        "Where it stands: Halfway",
        "Repos: /r/main",
        "https://example.test/pr/1",
        "T-42",
        "c1 · /r/1 · goal 1",
        "c2 · /r/2 · goal 2",
        "/n/notes",
        PRIVATE_RULE,
    ] {
        assert!(orch.contains(want), "missing {want:?} in\n{orch}");
    }
    for child_only in ["Your brief", "Current task", "Open question"] {
        assert!(!orch.contains(child_only), "{child_only} on orchestrator");
    }
    let child = render(&record(), &[kid(1)], Some(&me));
    for want in [
        "Your brief: Do the thing",
        "Current task: task one",
        "Open question: which way?",
    ] {
        assert!(child.contains(want), "missing {want:?} in\n{child}");
    }
}

#[test]
fn card_lists_worktrees() {
    let mut k = kid(1);
    k.worktrees = vec![
        "/r/1/.worktrees/p/c1".into(),
        "/r/1/.worktrees/p/c1.fix".into(),
        "/r/1/.worktrees/p/c1.docs".into(),
    ];
    let long = "line\n".repeat(200);
    let r = Record {
        what: long.clone(),
        why: long.clone(),
        status_text: long,
        ..record()
    };
    let kids: Vec<ChildLine> = std::iter::once(k).chain((2..9).map(kid)).collect();
    let card = render(&r, &kids, None);
    let line = card.lines().find(|l| l.starts_with("- c1 ")).unwrap();
    for p in [
        "/r/1/.worktrees/p/c1,",
        "/r/1/.worktrees/p/c1.fix,",
        "/r/1/.worktrees/p/c1.docs",
    ] {
        assert!(line.contains(p), "{p} in {line}");
    }
    assert_eq!(card.lines().filter(|l| l.contains("worktrees:")).count(), 1);
    assert!(card.lines().count() <= MAX_LINES, "{card}");
}

#[test]
fn card_without_private_has_no_rule() {
    let mut r = record();
    r.private.clear();
    assert!(!render(&r, &[], None).contains(PRIVATE_RULE));
}

#[test]
fn card_is_at_most_40_lines_however_big_the_record() {
    let long = "line\n".repeat(200);
    let many: Vec<String> = (0..50).map(|i| format!("item {i}\nsecond line")).collect();
    let r = Record {
        what: long.clone(),
        why: long.clone(),
        status_text: long.clone(),
        repos: many.clone(),
        links: many.clone(),
        tickets: many.clone(),
        private: many,
        ..Record::default()
    };
    let kids: Vec<ChildLine> = (0..8).map(kid).collect();
    let me = SelfPart {
        brief: long.clone(),
        current_task: Some(long.clone()),
        question: Some(long),
    };
    let card = render(&r, &kids, Some(&me));
    let n = card.lines().count();
    assert!(n <= MAX_LINES, "{n} lines:\n{card}");
    assert!(card.contains("more"), "cut lists say so");
}
