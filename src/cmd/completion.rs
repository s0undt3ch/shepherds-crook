//! `crook completion print <shell>` and the hidden `crook completion list <kind>` the scripts call
//! (ADR 0020, Q35).
//!
//! clap_complete's dynamic completion sits behind an unstable feature, so these are hand-written
//! scripts. They name the bare `crook`, never a path, so a script mise caches holds no version folder
//! that mise later prunes (ADR 0019, spike 18).

use clap::{CommandFactory, ValueEnum};

use super::Io;
use crate::cli::{Cli, Completion, Shell};
use crate::files::roster::{self, State};
use crate::{Ctx, Error, flock, names};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Kind {
    Slugs,
    Children,
}

pub fn run(ctx: &Ctx, io: &mut Io, what: &Completion) -> Result<(), Error> {
    match what {
        Completion::Print { shell } => {
            let commands: Vec<String> = Cli::command()
                .get_subcommands()
                .filter(|c| !c.is_hide_set())
                .map(|c| c.get_name().to_string())
                .collect();
            let script = script(*shell, &commands.join(" "));
            let _ = io.out.write_all(script.as_bytes());
            let _ = io.out.flush();
        }
        Completion::List { kind, flock } => {
            for candidate in candidates(ctx, *kind, flock.as_deref()) {
                io.say(candidate);
            }
        }
    }
    Ok(())
}

/// Never fails: it runs on every TAB, and a problem is simply no candidates.
pub fn candidates(ctx: &Ctx, kind: Kind, flock_slug: Option<&str>) -> Vec<String> {
    let mut found = match kind {
        Kind::Slugs => flock::list(ctx.env)
            .map(|all| all.into_iter().map(|f| f.slug).collect())
            .unwrap_or_default(),
        Kind::Children => children(ctx, flock_slug),
    };
    found.sort();
    found.dedup();
    found
}

fn children(ctx: &Ctx, flock_slug: Option<&str>) -> Vec<String> {
    let flocks = match flock::list(ctx.env) {
        Ok(all) => all,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for found in flocks {
        let wanted = match flock_slug {
            Some(slug) => names::validate_slug(slug).is_ok() && found.slug == slug,
            None => true,
        };
        if !wanted {
            continue;
        }
        let Ok(roster) = roster::load(&found.folder) else {
            continue;
        };
        if flock_slug.is_none() && roster.coordinator.state != State::Active {
            continue;
        }
        out.extend(roster.children.into_iter().map(|c| c.name));
    }
    out
}

fn script(shell: Shell, commands: &str) -> String {
    let template = match shell {
        Shell::Bash => BASH,
        Shell::Zsh => ZSH,
        Shell::Fish => FISH,
    };
    template.replace("@COMMANDS@", commands)
}

const BASH: &str = r#"# bash completion for crook
_crook() {
  local cur="${COMP_WORDS[COMP_CWORD]}" prev="" sub="${COMP_WORDS[1]}" i flock="" words=""
  [ "$COMP_CWORD" -gt 0 ] && prev="${COMP_WORDS[COMP_CWORD-1]}"
  for ((i = 1; i < COMP_CWORD; i++)); do
    [ "${COMP_WORDS[i]}" = "--flock" ] && flock="${COMP_WORDS[i+1]}"
  done
  if [ "$COMP_CWORD" -eq 1 ]; then
    words="@COMMANDS@"
  elif [ "$prev" = "--flock" ]; then
    words="$(crook completion list slugs 2>/dev/null)"
  elif [ "$prev" != "--model" ] && [ "$prev" != "--repo" ] && [ "$prev" != "--task" ]; then
    case "$sub" in
      open | close | status) words="$(crook completion list slugs 2>/dev/null)" ;;
      stop | resume | send | read)
        if [ -n "$flock" ]; then
          words="$(crook completion list children --flock "$flock" 2>/dev/null)"
        else
          words="$(crook completion list children 2>/dev/null)"
        fi
        ;;
      completion)
        if [ "$COMP_CWORD" -eq 2 ]; then words="print"; elif [ "$COMP_CWORD" -eq 3 ]; then words="bash zsh fish"; fi
        ;;
      hook) [ "$COMP_CWORD" -eq 2 ] && words="session-start stop user-prompt-submit" ;;
    esac
  fi
  COMPREPLY=($(compgen -W "$words" -- "$cur"))
}
complete -o default -F _crook crook
"#;

const ZSH: &str = r#"#compdef crook
# zsh completion for crook
_crook() {
  local -a candidates
  local sub flock="" i
  if (( CURRENT == 2 )); then
    candidates=(@COMMANDS@)
    compadd -a candidates
    return
  fi
  sub=$words[2]
  for (( i = 3; i < CURRENT; i++ )); do
    [[ $words[i] == --flock ]] && flock=$words[i+1]
  done
  case $words[CURRENT-1] in
    --flock)
      candidates=(${(f)"$(crook completion list slugs 2>/dev/null)"})
      compadd -a candidates
      return
      ;;
    --model | --repo | --task) return ;;
  esac
  case $sub in
    open | close | status)
      candidates=(${(f)"$(crook completion list slugs 2>/dev/null)"})
      ;;
    stop | resume | send | read)
      if [[ -n $flock ]]; then
        candidates=(${(f)"$(crook completion list children --flock $flock 2>/dev/null)"})
      else
        candidates=(${(f)"$(crook completion list children 2>/dev/null)"})
      fi
      ;;
    completion)
      if (( CURRENT == 3 )); then candidates=(print); elif (( CURRENT == 4 )); then candidates=(bash zsh fish); fi
      ;;
    hook)
      candidates=(session-start stop user-prompt-submit)
      ;;
  esac
  compadd -a candidates
}
if [ "$funcstack[1]" = "_crook" ]; then
  _crook "$@"
else
  compdef _crook crook
fi
"#;

const FISH: &str = r#"# fish completion for crook
function __crook_children
    set -l t (commandline -opc)
    set -l i (contains -i -- --flock $t)
    if test -n "$i"; and test (count $t) -gt $i
        crook completion list children --flock $t[(math $i + 1)] 2>/dev/null
    else
        crook completion list children 2>/dev/null
    end
end
complete -c crook -f
complete -c crook -n '__fish_use_subcommand' -a '@COMMANDS@'
complete -c crook -n '__fish_seen_subcommand_from open close status' -a '(crook completion list slugs 2>/dev/null)'
complete -c crook -n '__fish_seen_subcommand_from stop resume send read' -a '(__crook_children)'
complete -c crook -l flock -x -a '(crook completion list slugs 2>/dev/null)'
complete -c crook -n '__fish_seen_subcommand_from completion; and not __fish_seen_subcommand_from print' -a 'print'
complete -c crook -n '__fish_seen_subcommand_from print' -a 'bash zsh fish'
complete -c crook -n '__fish_seen_subcommand_from hook' -a 'session-start stop user-prompt-submit'
"#;
