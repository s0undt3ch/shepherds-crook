//! herdr, the optional terminal multiplexer. Only presence is known so far.

use crate::Env;

/// Whether crook runs inside a herdr pane, which sets `HERDR_ENV=1`. herdr
/// installed but not the one running crook counts as absent.
pub fn present(env: Env) -> bool {
    env("HERDR_ENV").as_deref() == Some("1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_only_for_herdr_env_one() {
        let with = |v: Option<&'static str>| {
            move |k: &str| v.filter(|_| k == "HERDR_ENV").map(str::to_string)
        };
        assert!(present(&with(Some("1"))));
        for v in [None, Some(""), Some("0"), Some("true")] {
            assert!(!present(&with(v)), "{v:?}");
        }
    }
}
