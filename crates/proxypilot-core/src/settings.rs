//! What the user chose, persisted as `settings.json`.
//!
//! Presets are stored only as "which ids are on", so improving a preset in a
//! new release reaches existing users instead of being frozen in their file.

use crate::catalog::{self, normalise_names};
use crate::domain::{covers, normalise_all, normalise_domain};
use crate::model::*;
use crate::presets::{Preset, presets, service_of};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

pub const SETTINGS_VERSION: u32 = 1;
pub const CUSTOM_PREFIX: &str = "custom-";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub processes: Vec<String>,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub version: u32,
    pub upstream: Option<Upstream>,
    /// Label of the client that owned the upstream port when it was chosen,
    /// used to follow that client to a new port after it restarts.
    pub upstream_client: Option<String>,
    /// The label above is a guess: the port's owner was hidden (macOS, a
    /// root-run core) and it names the one proxy client that was running.
    pub upstream_client_guessed: bool,
    pub routing_enabled: bool,
    pub enabled_presets: Vec<String>,
    pub custom_profiles: Vec<CustomProfile>,
    pub direct_processes: Vec<String>,
    pub intranet_domains: Vec<String>,
    pub dns_mode: DnsMode,
    pub log_level: LogLevel,
    pub autostart: bool,
    /// Look for a new release once a day. On by default, so files written
    /// before this setting existed turn it on too.
    pub check_updates: bool,
    /// Sites added to an item, preset or custom, because its pages needed
    /// them, by item id. Kept apart from the item's own sites so each can be
    /// taken out again.
    pub extra_domains: BTreeMap<String, Vec<String>>,
    /// Sites the user asked never to be suggested again.
    pub ignored_sites: Vec<String>,
    /// A system notification when a page seems to be missing sites. On by default.
    pub notify_missing_sites: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            version: 0,
            upstream: None,
            upstream_client: None,
            upstream_client_guessed: false,
            routing_enabled: false,
            enabled_presets: Vec::new(),
            custom_profiles: Vec::new(),
            direct_processes: Vec::new(),
            intranet_domains: Vec::new(),
            dns_mode: DnsMode::default(),
            log_level: LogLevel::default(),
            autostart: false,
            check_updates: true,
            extra_domains: BTreeMap::new(),
            ignored_sites: Vec::new(),
            notify_missing_sites: true,
        }
    }
}

/// What the config builder needs from the enabled profiles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Targets {
    pub processes: Vec<String>,
    pub domains: Vec<String>,
}

impl Settings {
    /// Read settings, never failing: a missing file is a first run, and a
    /// corrupt one is set aside so the user keeps a copy instead of losing it
    /// to the next save. The second value says what happened, for the log.
    pub fn load(path: &Path) -> (Settings, Option<String>) {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return (Settings::default(), None),
            Err(error) => return (Settings::default(), Some(format!("无法读取设置文件：{error}"))),
        };
        match serde_json::from_str::<Settings>(&text) {
            Ok(settings) => {
                let (settings, folded) = settings.sanitised();
                if folded.is_empty() {
                    return (settings, None);
                }
                // Saved right away, so the note is logged once rather than on every start.
                let saved = settings.save(path).map_or_else(|error| format!("，但没能保存：{error}"), |_| String::new());
                let names = folded.iter().map(|name| format!("“{name}”")).collect::<Vec<_>>().join("、");
                (settings, Some(format!("自己添加的网站已换成内置的 {names}，它们需要的其它网站也会一起走代理{saved}")))
            }
            Err(error) => {
                let aside = path.with_extension("json.corrupt");
                let moved = fs::rename(path, &aside).is_ok();
                let note = if moved { format!("，已另存为 {}", aside.display()) } else { String::new() };
                (Settings::default(), Some(format!("设置文件损坏（{error}）{note}")))
            }
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let mut copy = self.clone();
        copy.version = SETTINGS_VERSION;
        let mut text = serde_json::to_string_pretty(&copy).map_err(io::Error::other)?;
        text.push('\n');
        write_atomic(path, text.as_bytes())
    }

    /// Drop what a hand edit could have broken, keep everything else. Also
    /// returns the presets that took over the user's own sites (see `fold_into_presets`).
    fn sanitised(mut self) -> (Settings, Vec<String>) {
        self.upstream = self.upstream.and_then(|upstream| upstream.normalised().ok());
        let known: Vec<&str> = presets().iter().map(|preset| preset.id.as_str()).collect();
        self.enabled_presets.retain(|id| known.contains(&id.as_str()));
        self.enabled_presets.dedup();
        self.direct_processes = dedup_names(normalise_names(&self.direct_processes));
        self.intranet_domains = normalise_all(&self.intranet_domains);
        for profile in &mut self.custom_profiles {
            profile.processes = dedup_names(normalise_names(&profile.processes));
            profile.domains = normalise_all(&profile.domains);
        }
        self.custom_profiles.retain(|profile| {
            profile.id.starts_with(CUSTOM_PREFIX) && !(profile.processes.is_empty() && profile.domains.is_empty())
        });
        self.ignored_sites = normalise_all(&self.ignored_sites);
        for sites in self.extra_domains.values_mut() {
            *sites = normalise_all(sites.iter());
        }
        let folded = self.fold_into_presets();
        self.prune_extras();
        (self, folded)
    }

    /// Sites the user added before a preset covered them (x.com, before the X
    /// preset existed) become that preset, which also brings the sites the
    /// service needs (twimg.com) that the user could not have known about.
    /// Only entries made purely of one service's sites; anything with a
    /// program, or spanning services, is left as it is.
    fn fold_into_presets(&mut self) -> Vec<String> {
        let mut folded: Vec<String> = Vec::new();
        let mut index = 0;
        while index < self.custom_profiles.len() {
            let custom = &self.custom_profiles[index];
            let Some(preset) = sole_service(&custom.domains).filter(|_| custom.processes.is_empty()) else {
                index += 1;
                continue;
            };
            let custom = self.custom_profiles.remove(index);
            if custom.enabled {
                self.enable_preset(&preset.id);
            }
            if let Some(extras) = self.extra_domains.remove(&custom.id) {
                self.extra_domains.entry(preset.id.clone()).or_default().extend(extras);
            }
            if !folded.contains(&preset.name) {
                folded.push(preset.name.clone());
            }
        }
        folded
    }

    /// Extras of items that no longer exist, or that the item now covers itself.
    fn prune_extras(&mut self) {
        let profiles: Vec<(String, Vec<String>)> = self
            .profiles()
            .into_iter()
            .map(|profile| (profile.id, profile.domains))
            .collect();
        self.extra_domains.retain(|id, sites| {
            let Some((_, own)) = profiles.iter().find(|(known, _)| known == id) else { return false };
            let mut seen: Vec<String> = Vec::new();
            sites.retain(|site| {
                let keep = !own.iter().any(|suffix| covers(suffix, site)) && !seen.contains(site);
                if keep {
                    seen.push(site.clone());
                }
                keep
            });
            !sites.is_empty()
        });
    }

    fn enable_preset(&mut self, id: &str) {
        if !self.enabled_presets.iter().any(|existing| existing == id) {
            self.enabled_presets.push(id.to_owned());
        }
    }

    pub fn profiles(&self) -> Vec<Profile> {
        let extras = |id: &str| self.extra_domains.get(id).cloned().unwrap_or_default();
        let mut out: Vec<Profile> = presets()
            .iter()
            .map(|preset| Profile {
                id: preset.id.clone(),
                name: preset.name.clone(),
                description: preset.description.clone(),
                category: preset.category.clone(),
                keywords: preset.keywords.clone(),
                processes: preset.processes.clone(),
                domains: preset.domains.clone(),
                extra_domains: extras(&preset.id),
                enabled: self.enabled_presets.contains(&preset.id),
                source: ProfileSource::Preset,
            })
            .collect();
        out.extend(self.custom_profiles.iter().map(|custom| Profile {
            id: custom.id.clone(),
            name: custom.name.clone(),
            description: String::new(),
            category: String::new(),
            keywords: Vec::new(),
            processes: custom.processes.clone(),
            domains: custom.domains.clone(),
            extra_domains: extras(&custom.id),
            enabled: custom.enabled,
            source: ProfileSource::Custom,
        }));
        out
    }

    /// The enabled item that already sends `domain` through the proxy, if any.
    pub fn owner_of(&self, domain: &str) -> Option<Profile> {
        self.profiles()
            .into_iter()
            .find(|profile| profile.enabled && profile.all_domains().any(|suffix| covers(suffix, domain)))
    }

    /// Union of every enabled profile, sorted so the generated config (and the
    /// decision whether anything changed) does not depend on click order.
    pub fn targets(&self) -> Targets {
        let mut processes: Vec<String> = Vec::new();
        let mut domains: Vec<String> = Vec::new();
        for profile in self.profiles().into_iter().filter(|profile| profile.enabled) {
            domains.extend(profile.all_domains().cloned());
            processes.extend(profile.processes);
        }
        processes.sort_by_key(|name| name.to_lowercase());
        processes.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
        domains.sort();
        domains.dedup();
        Targets { processes, domains }
    }

    pub fn set_profile_enabled(&mut self, id: &str, enabled: bool) -> Result<(), String> {
        if let Some(custom) = self.custom_profiles.iter_mut().find(|custom| custom.id == id) {
            custom.enabled = enabled;
            return Ok(());
        }
        if !presets().iter().any(|preset| preset.id == id) {
            return Err("找不到这个分流项目".into());
        }
        self.enabled_presets.retain(|existing| existing != id);
        if enabled {
            self.enabled_presets.push(id.to_owned());
        }
        Ok(())
    }

    /// Add a site. One that belongs to a built-in service switches that
    /// service on instead, so its other sites come along: x.com alone leaves
    /// X without its pictures and scripts on twimg.com. Otherwise the site
    /// gets its own entry, or the entry that already has it is re-enabled. A
    /// site an enabled item already sends through the proxy is refused with
    /// that item's name, so the list does not fill with duplicates.
    pub fn add_site(&mut self, input: &str) -> Result<SiteAdded, String> {
        let domain = normalise_domain(input)?;
        if let Some(existing) = self.custom_profiles.iter_mut().find(|custom| custom.domains.contains(&domain)) {
            existing.enabled = true;
            return Ok(SiteAdded { domain, preset: None });
        }
        if let Some(owner) = self.owner_of(&domain) {
            return Err(format!("{domain} 已经在“{}”里走代理了，不用再添加", owner.name));
        }
        if let Some(preset) = service_of(&domain) {
            self.enable_preset(&preset.id);
            return Ok(SiteAdded { domain, preset: Some(preset.id.clone()) });
        }
        let id = self.next_custom_id();
        self.custom_profiles.push(CustomProfile {
            id,
            name: domain.clone(),
            processes: Vec::new(),
            domains: vec![domain.clone()],
            enabled: true,
        });
        Ok(SiteAdded { domain, preset: None })
    }

    /// Send sites an item's pages turned out to need through the proxy with
    /// it. A site of a built-in service switches that service on; any other
    /// joins the item's extras. Returns the sites that now go through the proxy.
    pub fn attach_sites(&mut self, id: &str, sites: &[String]) -> Result<Vec<String>, String> {
        if !self.profiles().iter().any(|profile| profile.id == id) {
            return Err("找不到这个分流项目".into());
        }
        let mut added = Vec::new();
        for site in sites {
            let Ok(domain) = normalise_domain(site) else { continue };
            if self.owner_of(&domain).is_some() {
                continue;
            }
            match service_of(&domain) {
                Some(preset) => self.enable_preset(&preset.id),
                None => self.extra_domains.entry(id.to_owned()).or_default().push(domain.clone()),
            }
            added.push(domain);
        }
        Ok(added)
    }

    /// Take one added site out of an item again.
    pub fn remove_extra_site(&mut self, id: &str, site: &str) -> Result<(), String> {
        let Some(sites) = self.extra_domains.get_mut(id).filter(|sites| sites.iter().any(|existing| existing == site)) else {
            return Err("这个网站已经不在列表里了".into());
        };
        sites.retain(|existing| existing != site);
        if sites.is_empty() {
            self.extra_domains.remove(id);
        }
        Ok(())
    }

    /// Never suggest these sites again.
    pub fn ignore_sites(&mut self, sites: &[String]) {
        for domain in normalise_all(sites) {
            if !self.ignored_sites.contains(&domain) {
                self.ignored_sites.push(domain);
            }
        }
    }

    /// Add an app by executable name, or on macOS by `.app` bundle. A full
    /// path is reduced to what the rules match on: the file name, or the
    /// bundle the path lies in.
    pub fn add_app(&mut self, input: &str) -> Result<String, String> {
        let name = crate::platform::program_of(input.trim().trim_end_matches('/')).trim().to_owned();
        if name.is_empty() {
            return Err("请选择一个程序".into());
        }
        let key = crate::platform::name_key(&name);
        if catalog::bypass_processes(&self.direct_processes, None).iter().any(|bypass| crate::platform::name_key(bypass) == key) {
            return Err(format!(
                "{} 是代理客户端、企业 VPN 或直连名单里的程序，它的流量必须直连，不能走代理",
                crate::platform::display_name(&name)
            ));
        }
        if let Some(existing) = self
            .custom_profiles
            .iter_mut()
            .find(|custom| custom.processes.iter().any(|process| crate::platform::name_key(process) == key))
        {
            existing.enabled = true;
            return Ok(name);
        }
        let owner = self.profiles().into_iter().find(|profile| {
            profile.enabled && profile.processes.iter().any(|process| crate::platform::name_key(process) == key)
        });
        if let Some(owner) = owner {
            return Err(format!(
                "{} 已经在“{}”里走代理了，不用再添加",
                crate::platform::display_name(&name),
                owner.name
            ));
        }
        let id = self.next_custom_id();
        let label = crate::platform::display_name(&name);
        self.custom_profiles.push(CustomProfile {
            id,
            name: label,
            processes: vec![name.clone()],
            domains: Vec::new(),
            enabled: true,
        });
        Ok(name)
    }

    pub fn remove_profile(&mut self, id: &str) -> Result<(), String> {
        let before = self.custom_profiles.len();
        self.custom_profiles.retain(|custom| custom.id != id);
        if self.custom_profiles.len() == before {
            return Err(if presets().iter().any(|preset| preset.id == id) {
                "预设项目不能删除，可以关闭它".into()
            } else {
                "找不到这个分流项目".into()
            });
        }
        self.extra_domains.remove(id);
        Ok(())
    }

    /// Apply the list and option fields. `autostart` is left to the caller,
    /// which has to register the task first and only record success.
    pub fn apply_patch(&mut self, patch: &SettingsPatch) {
        if let Some(list) = &patch.direct_processes {
            self.direct_processes = dedup_names(normalise_names(list));
        }
        if let Some(list) = &patch.intranet_domains {
            self.intranet_domains = normalise_all(list);
        }
        if let Some(mode) = patch.dns_mode {
            self.dns_mode = mode;
        }
        if let Some(level) = patch.log_level {
            self.log_level = level;
        }
        if let Some(check) = patch.check_updates {
            self.check_updates = check;
        }
        if let Some(notify) = patch.notify_missing_sites {
            self.notify_missing_sites = notify;
        }
    }

    fn next_custom_id(&self) -> String {
        let used: Vec<u32> = self
            .custom_profiles
            .iter()
            .filter_map(|custom| custom.id.strip_prefix(CUSTOM_PREFIX)?.parse().ok())
            .collect();
        let next = used.iter().max().map_or(1, |max| max + 1);
        format!("{CUSTOM_PREFIX}{next}")
    }
}

/// The one service every one of these sites belongs to.
fn sole_service(domains: &[String]) -> Option<&'static Preset> {
    let first = service_of(domains.first()?)?;
    domains.iter().all(|domain| service_of(domain).is_some_and(|preset| preset.id == first.id)).then_some(first)
}

fn dedup_names(names: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in names {
        if !out.iter().any(|existing| existing.eq_ignore_ascii_case(&name)) {
            out.push(name);
        }
    }
    out
}

/// Write to a sibling temp file, flush it to disk, then rename over the target,
/// so a crash mid-write leaves the old file rather than half of the new one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(match path.extension() {
        Some(ext) => format!("{}.tmp", ext.to_string_lossy()),
        None => "tmp".into(),
    });
    {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_is_a_first_run() {
        let dir = tempfile::tempdir().unwrap();
        let (settings, note) = Settings::load(&dir.path().join("settings.json"));
        assert_eq!(settings, Settings::default());
        assert!(note.is_none());
    }

    #[test]
    fn corrupt_file_is_set_aside_not_lost() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{ not json").unwrap();
        let (settings, note) = Settings::load(&path);
        assert_eq!(settings, Settings::default());
        assert!(note.unwrap().contains("损坏"));
        assert!(dir.path().join("settings.json.corrupt").exists());
    }

    #[test]
    fn round_trips_and_sanitises_hand_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut settings = Settings {
            upstream: Some(Upstream { kind: UpstreamKind::Socks5, host: "127.0.0.1".into(), port: 10808 }),
            ..Settings::default()
        };
        settings.set_profile_enabled("github", true).unwrap();
        settings.add_site("https://www.example.com/x").unwrap();
        settings.save(&path).unwrap();
        let (loaded, _) = Settings::load(&path);
        assert_eq!(loaded.version, SETTINGS_VERSION);
        assert_eq!(loaded.enabled_presets, vec!["github"]);
        assert_eq!(loaded.custom_profiles[0].domains, vec!["example.com"]);

        let edited = r#"{"enabledPresets":["github","gone"],"intranetDomains":["corp.cn","1.2.3.4"],
            "upstream":{"kind":"socks5","host":"not-an-ip","port":1},
            "customProfiles":[{"id":"custom-1","name":"x"},{"id":"evil","name":"y","domains":["a.com"]}]}"#;
        fs::write(&path, edited).unwrap();
        let (loaded, _) = Settings::load(&path);
        assert_eq!(loaded.enabled_presets, vec!["github"]);
        assert_eq!(loaded.intranet_domains, vec!["corp.cn"]);
        assert!(loaded.upstream.is_none());
        assert!(loaded.custom_profiles.is_empty());
    }

    #[test]
    fn update_checks_default_on_for_old_files_and_can_be_turned_off() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, r#"{"version":1,"routingEnabled":true}"#).unwrap();
        let (mut loaded, _) = Settings::load(&path);
        assert!(loaded.check_updates);
        loaded.apply_patch(&SettingsPatch { check_updates: Some(false), ..SettingsPatch::default() });
        loaded.save(&path).unwrap();
        assert!(!Settings::load(&path).0.check_updates);
    }

    #[test]
    fn targets_union_enabled_profiles_sorted_and_deduped() {
        let mut settings = Settings::default();
        // Added before the presets are on: afterwards they would be refused as covered.
        // pypi.org is in a collection, so it gets its own entry.
        settings.add_site("pypi.org").unwrap();
        settings.add_app(r"C:\Apps\Zed.exe").unwrap();
        settings.add_app("chatgpt.EXE").unwrap();
        settings.set_profile_enabled("openai", true).unwrap();
        settings.set_profile_enabled("dev-registries", true).unwrap();
        let targets = settings.targets();
        assert_eq!(targets.domains.iter().filter(|d| *d == "pypi.org").count(), 1);
        assert_eq!(targets.processes.iter().filter(|p| p.eq_ignore_ascii_case("chatgpt.exe")).count(), 1);
        assert!(targets.processes.contains(&"Zed.exe".to_owned()));
        let mut sorted = targets.domains.clone();
        sorted.sort();
        assert_eq!(targets.domains, sorted);
    }

    #[test]
    fn a_site_of_a_built_in_service_switches_the_service_on() {
        let mut settings = Settings::default();
        let added = settings.add_site("https://x.com/home").unwrap();
        assert_eq!(added, SiteAdded { domain: "x.com".into(), preset: Some("x".into()) });
        assert!(settings.custom_profiles.is_empty());
        assert!(settings.targets().domains.contains(&"twimg.com".to_owned()));
        // Its other sites are now covered.
        assert!(settings.add_site("abs.twimg.com").unwrap_err().contains("X (Twitter)"));
        // A site of no known service still gets its own entry.
        assert_eq!(settings.add_site("example.com").unwrap().preset, None);
        assert_eq!(settings.custom_profiles.len(), 1);
    }

    #[test]
    fn own_entries_made_before_a_preset_existed_become_the_preset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let file = r#"{"version":1,"customProfiles":[
            {"id":"custom-1","name":"x.com","domains":["x.com"],"enabled":true},
            {"id":"custom-2","name":"telegram.org","domains":["telegram.org"],"enabled":false},
            {"id":"custom-3","name":"Zed","processes":["Zed.exe"],"domains":["x.com"],"enabled":true},
            {"id":"custom-4","name":"example.com","domains":["example.com"],"enabled":true}],
            "extraDomains":{"custom-1":["example.net","twimg.com"],"custom-9":["gone.com"]}}"#;
        fs::write(&path, file).unwrap();
        let (loaded, note) = Settings::load(&path);
        assert!(note.unwrap().contains("“X (Twitter)”"));
        assert_eq!(loaded.enabled_presets, vec!["x"]);
        // The switched-off entry is folded without switching its service on.
        let ids: Vec<_> = loaded.custom_profiles.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["custom-3", "custom-4"]);
        // Extras move along; ones the preset covers, or of items gone, are dropped.
        assert_eq!(loaded.extra_domains.get("x"), Some(&vec!["example.net".to_owned()]));
        assert_eq!(loaded.extra_domains.len(), 1);
        // Saved, so the next start says nothing.
        assert!(Settings::load(&path).1.is_none());
    }

    #[test]
    fn suggested_sites_join_the_item_and_can_be_taken_out_again() {
        let mut settings = Settings::default();
        settings.add_site("example.com").unwrap();
        let sites = vec!["cdn.example.net".to_owned(), "twimg.com".to_owned(), "example.com".to_owned()];
        let added = settings.attach_sites("custom-1", &sites).unwrap();
        // example.com already went through the proxy; twimg.com brought X with it.
        assert_eq!(added, vec!["cdn.example.net", "twimg.com"]);
        assert_eq!(settings.enabled_presets, vec!["x"]);
        let profile = settings.profiles().into_iter().find(|p| p.id == "custom-1").unwrap();
        assert_eq!(profile.extra_domains, vec!["cdn.example.net"]);
        assert!(settings.targets().domains.contains(&"cdn.example.net".to_owned()));
        assert!(settings.attach_sites("custom-404", &sites).is_err());

        settings.remove_extra_site("custom-1", "cdn.example.net").unwrap();
        assert!(settings.extra_domains.is_empty());
        assert!(settings.remove_extra_site("custom-1", "cdn.example.net").is_err());
        settings.attach_sites("custom-1", &["a.example.org".to_owned()]).unwrap();
        settings.remove_profile("custom-1").unwrap();
        assert!(settings.extra_domains.is_empty());

        settings.ignore_sites(&["https://ads.example.org/x".to_owned(), "ads.example.org".to_owned()]);
        assert_eq!(settings.ignored_sites, vec!["ads.example.org"]);
    }

    #[test]
    fn adding_an_existing_site_re_enables_it() {
        let mut settings = Settings::default();
        settings.add_site("a.com").unwrap();
        settings.set_profile_enabled("custom-1", false).unwrap();
        settings.add_site("https://a.com/").unwrap();
        assert_eq!(settings.custom_profiles.len(), 1);
        assert!(settings.custom_profiles[0].enabled);
    }

    #[test]
    fn sites_and_apps_already_proxied_are_refused_with_the_reason() {
        let mut settings = Settings::default();
        settings.set_profile_enabled("github", true).unwrap();
        let error = settings.add_site("https://api.github.com/x").unwrap_err();
        assert!(error.contains("GitHub"), "{error}");
        settings.set_profile_enabled("openai", true).unwrap();
        assert!(settings.add_app(r"C:\Tools\codex.exe").unwrap_err().contains("ChatGPT / OpenAI"));
        // The macOS name of the same program.
        assert!(settings.add_app("/opt/homebrew/bin/codex").is_err());
        assert!(settings.custom_profiles.is_empty());
        // With the preset off, its site switches it back on, which then covers its subdomains.
        settings.set_profile_enabled("github", false).unwrap();
        assert_eq!(settings.add_site("github.com").unwrap().preset.as_deref(), Some("github"));
        assert!(settings.custom_profiles.is_empty());
        assert!(settings.add_site("gist.github.com").is_err());
        // An unknown site's own entry covers its subdomains the same way.
        settings.add_site("example.com").unwrap();
        assert!(settings.add_site("a.example.com").is_err());
    }

    #[test]
    fn proxy_cores_and_vpn_clients_cannot_be_proxied() {
        let mut settings = Settings::default();
        assert!(settings.add_app("xray.exe").is_err());
        assert!(settings.add_app("aTrust.exe").is_err());
        settings.direct_processes = vec!["Corp.exe".into()];
        assert!(settings.add_app("corp.EXE").is_err());
        // macOS names of the same programs.
        assert!(settings.add_app("xray").is_err());
        assert!(settings.add_app("/Applications/aTrust.app/").is_err());
    }

    #[test]
    fn macos_apps_are_added_as_bundles() {
        let mut settings = Settings::default();
        assert_eq!(settings.add_app("/Applications/Visual Studio Code.app/").unwrap(), "Visual Studio Code.app");
        assert_eq!(settings.custom_profiles[0].name, "Visual Studio Code");
        let helper = "/Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper.app/Contents/MacOS/Code Helper";
        assert_eq!(settings.add_app(helper).unwrap(), "Visual Studio Code.app");
        assert_eq!(settings.custom_profiles.len(), 1);
        assert_eq!(settings.add_app("/opt/homebrew/bin/codex").unwrap(), "codex");
    }

    #[test]
    fn presets_cannot_be_removed_and_custom_ids_do_not_collide() {
        let mut settings = Settings::default();
        assert!(settings.remove_profile("github").is_err());
        settings.add_site("a.com").unwrap();
        settings.add_site("b.com").unwrap();
        settings.remove_profile("custom-1").unwrap();
        settings.add_site("c.com").unwrap();
        let ids: Vec<_> = settings.custom_profiles.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["custom-2", "custom-3"]);
    }
}
