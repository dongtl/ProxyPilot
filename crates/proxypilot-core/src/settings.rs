//! What the user chose, persisted as `settings.json`.
//!
//! Presets are stored only as "which ids are on", so improving a preset in a
//! new release reaches existing users instead of being frozen in their file.

use crate::catalog::{self, normalise_names};
use crate::domain::{normalise_all, normalise_domain};
use crate::model::*;
use crate::presets::presets;
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub version: u32,
    pub upstream: Option<Upstream>,
    /// Label of the client that owned the upstream port when it was chosen,
    /// used to follow that client to a new port after it restarts.
    pub upstream_client: Option<String>,
    pub routing_enabled: bool,
    pub enabled_presets: Vec<String>,
    pub custom_profiles: Vec<CustomProfile>,
    pub direct_processes: Vec<String>,
    pub intranet_domains: Vec<String>,
    pub dns_mode: DnsMode,
    pub log_level: LogLevel,
    pub autostart: bool,
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
            Ok(settings) => (settings.sanitised(), None),
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

    /// Drop what a hand edit could have broken, keep everything else.
    fn sanitised(mut self) -> Settings {
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
        self
    }

    pub fn profiles(&self) -> Vec<Profile> {
        let mut out: Vec<Profile> = presets()
            .iter()
            .map(|preset| Profile {
                id: preset.id.clone(),
                name: preset.name.clone(),
                description: preset.description.clone(),
                processes: preset.processes.clone(),
                domains: preset.domains.clone(),
                enabled: self.enabled_presets.contains(&preset.id),
                source: ProfileSource::Preset,
            })
            .collect();
        out.extend(self.custom_profiles.iter().map(|custom| Profile {
            id: custom.id.clone(),
            name: custom.name.clone(),
            description: String::new(),
            processes: custom.processes.clone(),
            domains: custom.domains.clone(),
            enabled: custom.enabled,
            source: ProfileSource::Custom,
        }));
        out
    }

    /// Union of every enabled profile, sorted so the generated config (and the
    /// decision whether anything changed) does not depend on click order.
    pub fn targets(&self) -> Targets {
        let mut processes: Vec<String> = Vec::new();
        let mut domains: Vec<String> = Vec::new();
        for profile in self.profiles().into_iter().filter(|profile| profile.enabled) {
            processes.extend(profile.processes);
            domains.extend(profile.domains);
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

    /// Add a site, or re-enable the custom entry that already has it. Returns the domain.
    pub fn add_site(&mut self, input: &str) -> Result<String, String> {
        let domain = normalise_domain(input)?;
        if let Some(existing) = self.custom_profiles.iter_mut().find(|custom| custom.domains.contains(&domain)) {
            existing.enabled = true;
            return Ok(domain);
        }
        let id = self.next_custom_id();
        self.custom_profiles.push(CustomProfile {
            id,
            name: domain.clone(),
            processes: Vec::new(),
            domains: vec![domain.clone()],
            enabled: true,
        });
        Ok(domain)
    }

    /// Add an app by executable name. A full path is reduced to its file name,
    /// because that is what sing-box matches on.
    pub fn add_app(&mut self, input: &str) -> Result<String, String> {
        let name = input.trim().rsplit(['\\', '/']).next().unwrap_or("").trim().to_owned();
        if name.is_empty() {
            return Err("请选择一个程序".into());
        }
        if catalog::bypass_processes(&self.direct_processes, None).iter().any(|bypass| bypass.eq_ignore_ascii_case(&name)) {
            return Err(format!("{name} 是代理客户端、企业 VPN 或直连名单里的程序，它的流量必须直连，不能走代理"));
        }
        if let Some(existing) = self
            .custom_profiles
            .iter_mut()
            .find(|custom| custom.processes.iter().any(|process| process.eq_ignore_ascii_case(&name)))
        {
            existing.enabled = true;
            return Ok(name);
        }
        let id = self.next_custom_id();
        let label = name.strip_suffix(".exe").or_else(|| name.strip_suffix(".EXE")).unwrap_or(&name).to_owned();
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
    fn targets_union_enabled_profiles_sorted_and_deduped() {
        let mut settings = Settings::default();
        settings.set_profile_enabled("openai", true).unwrap();
        settings.add_site("openai.com").unwrap();
        settings.add_app(r"C:\Apps\Zed.exe").unwrap();
        settings.add_app("chatgpt.EXE").unwrap();
        let targets = settings.targets();
        assert_eq!(targets.domains.iter().filter(|d| *d == "openai.com").count(), 1);
        assert_eq!(targets.processes.iter().filter(|p| p.eq_ignore_ascii_case("chatgpt.exe")).count(), 1);
        assert!(targets.processes.contains(&"Zed.exe".to_owned()));
        let mut sorted = targets.domains.clone();
        sorted.sort();
        assert_eq!(targets.domains, sorted);
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
    fn proxy_cores_and_vpn_clients_cannot_be_proxied() {
        let mut settings = Settings::default();
        assert!(settings.add_app("xray.exe").is_err());
        assert!(settings.add_app("aTrust.exe").is_err());
        settings.direct_processes = vec!["Corp.exe".into()];
        assert!(settings.add_app("corp.EXE").is_err());
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
