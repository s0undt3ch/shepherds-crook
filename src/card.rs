//! The project card: the short, slow-changing summary every session gets.
//!
//! Each field and list has a line budget, so the worst case stays under
//! `MAX_LINES` however much the record holds.

use crate::project::Record;

pub const MAX_LINES: usize = 40;

pub const PRIVATE_RULE: &str = "use freely for work; anything from these leaving through a tool call raises a prompt; never reference them in public without asking";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildLine {
    pub id: String,
    pub repo: String,
    pub goal: String,
}

/// The parts only a child's own card has.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelfPart {
    pub brief: String,
    pub current_task: Option<String>,
    pub question: Option<String>,
}

const TEXT_LINES: usize = 4;
const WHAT_WHY_LINES: usize = 2;
const LIST_SHOWN: usize = 3;
const CHILDREN_SHOWN: usize = 5;
const SELF_LINES: usize = 3;
const TASK_LINES: usize = 2;

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `Label: text`, with the text cut to `max` lines.
fn push_text(out: &mut Vec<String>, label: &str, text: &str, max: usize) {
    let lines: Vec<&str> = text.trim().lines().collect();
    if lines.is_empty() {
        return;
    }
    for (i, line) in lines.iter().take(max).enumerate() {
        if i == 0 {
            out.push(format!("{label}: {line}"));
        } else {
            out.push((*line).to_string());
        }
    }
    if lines.len() > max
        && let Some(last) = out.last_mut()
    {
        last.push_str(" …");
    }
}

fn push_list(out: &mut Vec<String>, header: &str, items: &[String], shown: usize) {
    if items.is_empty() {
        return;
    }
    out.push(header.to_string());
    for item in items.iter().take(shown) {
        out.push(format!("- {}", one_line(item)));
    }
    if items.len() > shown {
        out.push(format!("- … {} more", items.len() - shown));
    }
}

/// `me` is `None` for the orchestrator's card.
pub fn render(r: &Record, kids: &[ChildLine], me: Option<&SelfPart>) -> String {
    let mut out = vec!["Project card".to_string()];
    push_text(&mut out, "What", &r.what, WHAT_WHY_LINES);
    push_text(&mut out, "Why", &r.why, WHAT_WHY_LINES);
    push_text(&mut out, "Where it stands", &r.status_text, TEXT_LINES);
    push_list(&mut out, "Links:", &r.links, LIST_SHOWN);
    push_list(&mut out, "Tickets:", &r.tickets, LIST_SHOWN);
    let lines: Vec<String> = kids
        .iter()
        .map(|k| format!("{} · {} · {}", k.id, k.repo, k.goal))
        .collect();
    push_list(&mut out, "Children:", &lines, CHILDREN_SHOWN);
    if !r.private.is_empty() {
        let mut items = Vec::new();
        push_list(
            &mut items,
            &format!("Private locations ({PRIVATE_RULE}):"),
            &r.private,
            LIST_SHOWN,
        );
        out.extend(items);
    }
    if let Some(me) = me {
        push_text(&mut out, "Your brief", &me.brief, SELF_LINES);
        if let Some(t) = &me.current_task {
            push_text(&mut out, "Current task", t, TASK_LINES);
        }
        if let Some(q) = &me.question {
            push_text(&mut out, "Open question", q, TASK_LINES);
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Record {
        Record {
            what: "A thing".into(),
            why: "Because".into(),
            status_text: "Halfway".into(),
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
}
