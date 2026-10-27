//! Profiles that ship with the program. The data lives in `presets/presets.json`
//! so adding a service is a data change anyone can review, not a code change.

use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub processes: Vec<String>,
    pub domains: Vec<String>,
}

const PRESETS_JSON: &str = include_str!("../presets/presets.json");

pub fn presets() -> &'static [Preset] {
    static PRESETS: OnceLock<Vec<Preset>> = OnceLock::new();
    // The file is compiled in and covered by the test below, so a parse failure
    // here can only be a build that skipped the tests.
    PRESETS.get_or_init(|| serde_json::from_str(PRESETS_JSON).expect("presets.json is invalid"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::normalise_domain;
    use std::collections::HashSet;

    #[test]
    fn presets_parse_with_unique_ids_and_normalised_domains() {
        let all = presets();
        assert!(!all.is_empty());
        let ids: HashSet<_> = all.iter().map(|preset| preset.id.as_str()).collect();
        assert_eq!(ids.len(), all.len(), "duplicate preset id");
        for preset in all {
            assert!(!preset.domains.is_empty() || !preset.processes.is_empty(), "{} is empty", preset.id);
            for domain in &preset.domains {
                assert_eq!(normalise_domain(domain).as_deref(), Ok(domain.as_str()), "{} in {}", domain, preset.id);
            }
            assert!(!preset.id.starts_with("custom-"), "custom- prefix is reserved");
        }
    }
}
