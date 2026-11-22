//! Entry-point gates: who may call which kind of verb.

use crate::{Env, Error, paths};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    User,
    Mod,
    Hook,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Orchestrator,
    Child,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub slug: String,
    pub role: Role,
    pub child: Option<String>,
}

impl Identity {
    fn none() -> Self {
        Identity {
            slug: String::new(),
            role: Role::None,
            child: None,
        }
    }
}

/// Only the value `1` counts: the fallback in Spike 9 sets it to the empty string.
fn in_claude_code(env: Env) -> bool {
    env("CLAUDECODE").as_deref() == Some("1")
}

fn non_empty(env: Env, key: &str) -> Option<String> {
    env(key).filter(|v| !v.is_empty())
}

fn parse_role(s: &str) -> Option<Role> {
    match s {
        "orchestrator" => Some(Role::Orchestrator),
        "child" => Some(Role::Child),
        _ => None,
    }
}

/// Resolves the identity from `CROOK_*`; `None` when it is absent or inconsistent.
fn identity(env: Env) -> Option<Identity> {
    let role = parse_role(&non_empty(env, "CROOK_ROLE")?)?;
    let slug = non_empty(env, "CROOK_PROJECT")?;
    paths::valid_slug(&slug).ok()?;
    let child = non_empty(env, "CROOK_CHILD");
    if role == Role::Child && child.is_none() {
        return None;
    }
    let child = if role == Role::Child { child } else { None };
    Some(Identity { slug, role, child })
}

pub fn check(entry: Entry, env: Env) -> Result<Identity, Error> {
    match entry {
        Entry::User => {
            if in_claude_code(env) {
                return Err(Error::Refused(
                    "user verbs cannot run inside Claude Code (CLAUDECODE=1)".into(),
                ));
            }
            // Any value counts, even an empty one: a session's environment set it.
            if env("CROOK_ROLE").is_some() {
                return Err(Error::Refused(
                    "user verbs cannot run inside a crook session (CROOK_ROLE is set)".into(),
                ));
            }
            Ok(Identity::none())
        }
        Entry::Mod => {
            let role_raw = non_empty(env, "CROOK_ROLE")
                .ok_or_else(|| Error::Refused("mod verbs need CROOK_ROLE".into()))?;
            if in_claude_code(env) {
                return Err(Error::Refused(
                    "mod verbs cannot run from a Claude Code shell (CLAUDECODE=1)".into(),
                ));
            }
            let role = parse_role(&role_raw).ok_or_else(|| {
                Error::Refused(format!(
                    "CROOK_ROLE must be orchestrator or child, not {role_raw:?}"
                ))
            })?;
            let slug = non_empty(env, "CROOK_PROJECT")
                .ok_or_else(|| Error::Refused("mod verbs need CROOK_PROJECT".into()))?;
            paths::valid_slug(&slug)?;
            let child =
                match role {
                    Role::Child => Some(non_empty(env, "CROOK_CHILD").ok_or_else(|| {
                        Error::Refused("CROOK_ROLE=child needs CROOK_CHILD".into())
                    })?),
                    _ => None,
                };
            Ok(Identity { slug, role, child })
        }
        Entry::Hook => Ok(identity(env).unwrap_or_else(Identity::none)),
    }
}

#[cfg(test)]
mod tests;
