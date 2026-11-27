//! Helpers shared by unit tests.

use std::collections::HashMap;

/// An injected environment holding exactly `pairs`.
pub fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    move |key| map.get(key).cloned()
}
