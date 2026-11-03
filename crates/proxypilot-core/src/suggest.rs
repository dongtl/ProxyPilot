//! Sites a page needed but could not reach directly, offered to the item that
//! sent the page through the proxy: x.com went through, its pictures on
//! twimg.com did not. Nothing is changed until the user says so, because a
//! company site that only works while the VPN is up looks just as blocked
//! while it is down.

use crate::domain::{normalise_domain, site_of};
use crate::model::Suggestion;
use crate::settings::Settings;
use crate::traffic::Stall;
use std::collections::{HashMap, HashSet};

const MAX_SITES: usize = 8;
const MAX_ITEMS: usize = 5;
/// One system notification per item at most this often.
const NOTIFY_EVERY_MS: u64 = 10 * 60 * 1000;

#[derive(Default)]
pub struct Suggestions {
    pending: Vec<Suggestion>,
    /// Closed with "关闭" this run; asked about again after a restart.
    dismissed: HashSet<String>,
    notified_at: HashMap<String, u64>,
}

/// What `add` changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Added {
    /// New sites were offered; the window should be told.
    pub changed: bool,
    /// Offers worth a system notification now.
    pub notify: Vec<Suggestion>,
}

/// What to offer for a blocked host: its site, or the host itself when the
/// site would be wider than one owner (a host on a shared cloud platform).
fn offered_site(host: &str) -> Option<String> {
    site_of(host).or_else(|| normalise_domain(host).ok())
}

impl Suggestions {
    pub fn add(&mut self, stalls: &[Stall], settings: &Settings, now_ms: u64) -> Added {
        let mut added = Added::default();
        let mut touched: Vec<String> = Vec::new();
        for stall in stalls {
            let Some(parent) = stall.parent_host.as_deref().and_then(|host| settings.owner_of(host)) else { continue };
            let Some(site) = offered_site(&stall.host) else { continue };
            let ignored = settings.ignored_sites.iter().any(|suffix| crate::domain::covers(suffix, &site));
            if ignored || self.dismissed.contains(&site) || settings.owner_of(&site).is_some() {
                continue;
            }
            let index = match self.pending.iter().position(|pending| pending.profile_id == parent.id) {
                Some(index) => index,
                None if self.pending.len() < MAX_ITEMS => {
                    self.pending.push(Suggestion { profile_id: parent.id.clone(), profile_name: parent.name.clone(), sites: Vec::new() });
                    self.pending.len() - 1
                }
                None => continue,
            };
            let sites = &mut self.pending[index].sites;
            if sites.contains(&site) || sites.len() >= MAX_SITES {
                continue;
            }
            sites.push(site);
            added.changed = true;
            if !touched.contains(&parent.id) {
                touched.push(parent.id);
            }
        }
        for id in touched {
            let due = self.notified_at.get(&id).is_none_or(|at| now_ms.saturating_sub(*at) >= NOTIFY_EVERY_MS);
            if due && let Some(pending) = self.pending.iter().find(|pending| pending.profile_id == id) {
                self.notified_at.insert(id, now_ms);
                added.notify.push(pending.clone());
            }
        }
        added
    }

    /// What to show now: sites that went through the proxy meanwhile, or
    /// were ignored, drop out, and so do items switched off since.
    pub fn current(&mut self, settings: &Settings) -> Vec<Suggestion> {
        let profiles = settings.profiles();
        self.pending.retain_mut(|pending| {
            let enabled = profiles.iter().any(|profile| profile.id == pending.profile_id && profile.enabled);
            pending.sites.retain(|site| {
                settings.owner_of(site).is_none()
                    && !settings.ignored_sites.iter().any(|suffix| crate::domain::covers(suffix, site))
            });
            enabled && !pending.sites.is_empty()
        });
        self.pending.clone()
    }

    /// Hand over the offer for an item, to accept or ignore.
    pub fn take(&mut self, profile_id: &str) -> Option<Suggestion> {
        let index = self.pending.iter().position(|pending| pending.profile_id == profile_id)?;
        Some(self.pending.remove(index))
    }

    /// "关闭": not now, and not again this run.
    pub fn dismiss(&mut self, profile_id: &str) {
        if let Some(pending) = self.take(profile_id) {
            self.dismissed.extend(pending.sites);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stall(host: &str, parent: &str) -> Stall {
        Stall { process: "chrome.exe".into(), host: host.into(), parent_host: Some(parent.into()) }
    }

    fn with_example_on() -> Settings {
        let mut settings = Settings::default();
        settings.add_site("example.com").unwrap();
        settings
    }

    #[test]
    fn blocked_parts_of_a_proxied_page_are_offered_to_its_item() {
        let mut settings = with_example_on();
        settings.set_profile_enabled("github", true).unwrap();
        let mut suggestions = Suggestions::default();
        let stalls = [
            stall("img.cdn-example.net", "www.example.com"),
            stall("api.cdn-example.net", "example.com"),
            stall("x.cloudfront.net", "example.com"),
            stall("avatars.example-git.io", "github.com"),
            // Not part of a proxied page: nothing to say.
            stall("blocked.example.org", "news.example.cn"),
        ];
        let added = suggestions.add(&stalls, &settings, 1_000);
        assert!(added.changed);
        let current = suggestions.current(&settings);
        assert_eq!(current.len(), 2);
        assert_eq!(current[0].profile_id, "custom-1");
        // The site, except on a shared platform where only the host is safe.
        assert_eq!(current[0].sites, vec!["cdn-example.net", "x.cloudfront.net"]);
        assert_eq!(current[1].sites, vec!["example-git.io"]);
        assert_eq!(added.notify.len(), 2);

        // Seen again a minute later: nothing new, no second notification.
        let again = suggestions.add(&stalls, &settings, 61_000);
        assert_eq!(again, Added::default());
        // Something new for the same item soon after updates the window but does not notify.
        let more = suggestions.add(&[stall("more.example-media.com", "example.com")], &settings, 62_000);
        assert!(more.changed && more.notify.is_empty());
    }

    #[test]
    fn dismissed_ignored_and_now_proxied_sites_are_not_offered() {
        let mut settings = with_example_on();
        let mut suggestions = Suggestions::default();
        suggestions.add(&[stall("a.cdn-example.net", "example.com")], &settings, 0);
        suggestions.dismiss("custom-1");
        assert!(suggestions.current(&settings).is_empty());
        assert!(!suggestions.add(&[stall("b.cdn-example.net", "example.com")], &settings, 1).changed);

        // Offers are whole sites, so that is what "不再提醒" stores.
        settings.ignore_sites(&["example.org".to_owned()]);
        assert!(!suggestions.add(&[stall("ads.example.org", "example.com")], &settings, 2).changed);

        suggestions.add(&[stall("img.example-media.com", "example.com")], &settings, 3);
        let taken = suggestions.take("custom-1").unwrap();
        settings.attach_sites(&taken.profile_id, &taken.sites).unwrap();
        assert!(!suggestions.add(&[stall("img.example-media.com", "example.com")], &settings, 4).changed);

        // An item switched off takes its offer with it.
        suggestions.add(&[stall("c.example-video.com", "example.com")], &settings, 5);
        settings.set_profile_enabled("custom-1", false).unwrap();
        assert!(suggestions.current(&settings).is_empty());
    }
}
