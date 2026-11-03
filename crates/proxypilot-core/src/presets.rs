//! Profiles that ship with the program. The data lives in `presets/presets.json`
//! so adding a service is a data change anyone can review, not a code change.
//! Most domain lists were checked against v2fly/domain-list-community (MIT),
//! keeping only the sites a service needs to work.

use crate::domain::covers;
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    /// Group on the profiles page, one of [`CATEGORIES`].
    pub category: String,
    /// Other names people search by: "推特" finds X.
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Several unrelated services in one card, like the developer registries:
    /// adding one of their sites does not mean wanting all of them.
    #[serde(default)]
    pub collection: bool,
    pub description: String,
    pub processes: Vec<String>,
    pub domains: Vec<String>,
}

/// In the order the profiles page shows them.
pub const CATEGORIES: &[&str] = &["ai", "web", "social", "media", "dev"];

const PRESETS_JSON: &str = include_str!("../presets/presets.json");

pub fn presets() -> &'static [Preset] {
    static PRESETS: OnceLock<Vec<Preset>> = OnceLock::new();
    // The file is compiled in and covered by the test below, so a parse failure
    // here can only be a build that skipped the tests.
    PRESETS.get_or_init(|| serde_json::from_str(PRESETS_JSON).expect("presets.json is invalid"))
}

/// The one service a site belongs to, so adding "x.com" or "twimg.com" means
/// all of X. The longest matching entry wins, which makes `grok.x.com` Grok
/// rather than X; a site two services list at the same length (`ggpht.com` is
/// both Google's and YouTube's) belongs to neither.
pub fn service_of(domain: &str) -> Option<&'static Preset> {
    let mut best: Option<(&Preset, usize)> = None;
    let mut tied = false;
    for preset in presets().iter().filter(|preset| !preset.collection) {
        let Some(length) = preset.domains.iter().filter(|entry| covers(entry, domain)).map(String::len).max() else {
            continue;
        };
        match best {
            Some((_, current)) if length < current => {}
            Some((_, current)) if length == current => tied = true,
            _ => {
                best = Some((preset, length));
                tied = false;
            }
        }
    }
    best.filter(|_| !tied).map(|(preset, _)| preset)
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
            assert!(CATEGORIES.contains(&preset.category.as_str()), "{} has category {}", preset.id, preset.category);
        }
    }

    #[test]
    fn every_service_is_found_by_its_main_site() {
        for preset in presets().iter().filter(|preset| !preset.collection && !preset.domains.is_empty()) {
            let found = service_of(&preset.domains[0]).map(|found| found.id.as_str());
            assert_eq!(found, Some(preset.id.as_str()), "{}", preset.domains[0]);
        }
    }

    #[test]
    fn a_site_belongs_to_the_most_specific_service_or_to_none() {
        let id = |domain: &str| service_of(domain).map(|preset| preset.id.as_str());
        assert_eq!(id("abs.twimg.com"), Some("x"));
        assert_eq!(id("grok.x.com"), Some("grok"));
        assert_eq!(id("youtubei.googleapis.com"), Some("youtube"));
        assert_eq!(id("www.googleapis.com"), Some("google"));
        // Shared by two services at the same depth.
        assert_eq!(id("ggpht.com"), None);
        assert_eq!(id("fbcdn.net"), None);
        // A collection's sites are separate services.
        assert_eq!(id("pypi.org"), None);
        // A wider name than any entry, and unknown sites.
        assert_eq!(id("cloudflare.com"), None);
        assert_eq!(id("example.com"), None);
    }
}
