//! Slugs, child names and every string derived from them: hc names, labels, branches, worktree folders
//! and herdr ids. No other module formats one of these (ADR 0015, 0021, 0023; Q3, Q12, Q45).

use crate::Error;

#[cfg(test)]
mod tests;

pub const MAX_NAME_LEN: usize = 40;

fn validate(kind: &'static str, name: &str) -> Result<(), Error> {
    let bad = |reason| {
        Err(Error::InvalidName {
            kind,
            name: name.to_string(),
            reason,
        })
    };
    if name.is_empty() {
        return bad("it is empty");
    }
    if name.len() > MAX_NAME_LEN {
        return bad("it is longer than 40 characters");
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    {
        return bad("only lower-case letters, digits and `-` are allowed");
    }
    if name.starts_with('-') || name.ends_with('-') {
        return bad("it must start and end with a letter or digit");
    }
    if name.contains("--") {
        return bad("it must not contain `--`");
    }
    Ok(())
}

/// `^[a-z0-9]+(-[a-z0-9]+)*$`, at most 40 characters (Q12, Q45).
pub fn validate_slug(slug: &str) -> Result<(), Error> {
    validate("slug", slug)
}

/// The same rule as a slug (Q45).
pub fn validate_child(child: &str) -> Result<(), Error> {
    validate("child name", child)
}

/// The flock's folder name, `<yyyy-mm-dd>-<slug>` (ADR 0004). The date is not checked here.
pub fn folder_name(date: &str, slug: &str) -> String {
    format!("{date}-{slug}")
}

/// The coordinator's hc name (Q3).
pub fn hc_coordinator(slug: &str) -> String {
    slug.to_string()
}

/// A child's hc name (Q3).
pub fn hc_child(slug: &str, child: &str) -> String {
    format!("{slug}.{child}")
}

/// The herdr space id (ADR 0015, 0021).
pub fn herdr_space(slug: &str) -> String {
    format!("crook-{slug}")
}

/// The coordinator's herdr tab id (ADR 0021).
pub fn herdr_tab(slug: &str) -> String {
    format!("crook-{slug}-coord")
}

/// Labels on the coordinator's terminal, as `key=value` (ADR 0019, 0020, 0021).
pub fn coordinator_labels(slug: &str, version: &str) -> Vec<String> {
    vec![
        format!("crook.flock={slug}"),
        format!("crook.version={version}"),
        format!("herdr.space={}", herdr_space(slug)),
        format!("herdr.tab={}", herdr_tab(slug)),
    ]
}

/// Labels on a child's terminal: no `herdr.*` (ADR 0021, Q4).
pub fn child_labels(slug: &str, child: &str, version: &str) -> Vec<String> {
    vec![
        format!("crook.flock={slug}"),
        format!("crook.version={version}"),
        format!("crook.child={child}"),
    ]
}

/// `crook/<slug>/<child>` (ADR 0023).
pub fn branch(slug: &str, child: &str) -> String {
    format!("crook/{slug}/{child}")
}

/// The worktree's folder name under `<repo>/.worktrees/`, `<slug>-<child>` (ADR 0023).
pub fn worktree_dir(slug: &str, child: &str) -> String {
    format!("{slug}-{child}")
}
