//! How a program is named on each system, and what that means for rules.
//!
//! Windows programs are matched by executable name (`Code.exe`). On macOS a
//! program is an `.app` bundle whose network traffic often comes from helper
//! processes inside it ("Code Helper", "Google Chrome Helper"), so a bundle
//! is matched by path: every executable under `Name.app/Contents/`. Plain
//! command-line programs (node, codex) are matched by file name everywhere.
//!
//! Everything here is a pure function of an [`Os`], so both systems' rules
//! are tested on whichever one runs the tests.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    MacOs,
}

impl Os {
    /// The system this build runs on. Anything that is not Windows behaves
    /// like macOS: no `.exe` suffixes, `/` paths.
    pub const CURRENT: Os = if cfg!(windows) { Os::Windows } else { Os::MacOs };
}

fn ends_with_ignore_case(text: &str, suffix: &str) -> bool {
    text.len() >= suffix.len()
        && text.is_char_boundary(text.len() - suffix.len())
        && text[text.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

fn strip_suffix_ignore_case<'a>(text: &'a str, suffix: &str) -> &'a str {
    if ends_with_ignore_case(text, suffix) { &text[..text.len() - suffix.len()] } else { text }
}

/// `Visual Studio Code.app` and the like: a macOS bundle rather than a process name.
pub fn is_bundle(name: &str) -> bool {
    name.len() > 4 && ends_with_ignore_case(name, ".app")
}

/// Key for comparing program names across systems: lower case, without
/// `.exe`, so the catalogue's `xray.exe` also covers macOS's `xray`.
pub fn name_key(name: &str) -> String {
    strip_suffix_ignore_case(name.trim(), ".exe").to_lowercase()
}

/// The outermost `.app` a path lies inside, e.g. `Visual Studio Code.app`
/// for `/Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper.app/...`.
/// The outermost one, because nested helper bundles belong to the app the user knows.
pub fn bundle_of(path: &str) -> Option<String> {
    path.split('/').find(|part| is_bundle(part)).map(str::to_owned)
}

/// sing-box appends " (user)" to process paths on some systems; app names
/// can contain brackets too, so only a trailing group is dropped.
fn strip_owner(path: &str) -> &str {
    let path = path.trim();
    match path.rfind(" (") {
        Some(at) if path.ends_with(')') && !path[at..].contains('/') && !path[at..].contains('\\') => &path[..at],
        _ => path,
    }
}

/// What a user calls the program behind a process path: its `.app` when it
/// is inside one, otherwise its file name. Empty when the path is empty.
pub fn program_of(path: &str) -> String {
    let path = strip_owner(path);
    if let Some(bundle) = bundle_of(path) {
        return bundle;
    }
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_owned()
}

/// How a program is shown: without `.exe` or `.app`.
pub fn display_name(name: &str) -> String {
    let name = name.trim();
    let name = if is_bundle(name) { strip_suffix_ignore_case(name, ".app") } else { strip_suffix_ignore_case(name, ".exe") };
    name.to_owned()
}

/// Names for sing-box `process_name` on `os`. Bundles are left to
/// [`bundle_patterns`]; on macOS `.exe` is dropped, so `xray.exe` matches
/// the `xray` binary. Duplicates (by [`name_key`]) are removed, first kept.
pub fn process_names(names: &[String], os: Os) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for name in names.iter().map(|name| name.trim()).filter(|name| !name.is_empty() && !is_bundle(name)) {
        let name = match os {
            Os::Windows => name.to_owned(),
            Os::MacOs => strip_suffix_ignore_case(name, ".exe").to_owned(),
        };
        if !name.is_empty() && seen.insert(name_key(&name)) {
            out.push(name);
        }
    }
    out
}

/// sing-box `process_path_regex` entries matching every executable inside
/// each bundle. Empty on Windows, where there are no bundles.
pub fn bundle_patterns(names: &[String], os: Os) -> Vec<String> {
    if os == Os::Windows {
        return Vec::new();
    }
    let mut seen = std::collections::HashSet::new();
    names
        .iter()
        .map(|name| name.trim())
        .filter(|name| is_bundle(name) && !name.contains('/'))
        .filter(|name| seen.insert(name.to_lowercase()))
        .map(|name| format!("(?i)/{}/Contents/", regex_escape(name)))
        .collect()
}

/// Escape for Go's RE2 syntax, which treats the same characters as special
/// as Perl does.
pub fn regex_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4);
    for c in text.chars() {
        if "\\.+*?()|[]{}^$".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Paths of system programs nobody routes; hidden from the running-program list.
const MAC_SYSTEM_PREFIXES: &[&str] = &["/System/", "/usr/libexec/", "/usr/sbin/", "/usr/bin/", "/sbin/", "/bin/", "/Library/Apple/"];

/// Whether a running program at `path` is worth offering in "add app".
pub fn is_user_program(path: &str, os: Os) -> bool {
    match os {
        Os::Windows => true,
        Os::MacOs => !MAC_SYSTEM_PREFIXES.iter().any(|prefix| path.starts_with(prefix)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn bundles_and_programs_are_found_in_paths() {
        let helper = "/Applications/Visual Studio Code.app/Contents/Frameworks/Code Helper (Plugin).app/Contents/MacOS/Code Helper (Plugin)";
        assert_eq!(bundle_of(helper).as_deref(), Some("Visual Studio Code.app"));
        assert_eq!(program_of(helper), "Visual Studio Code.app");
        assert_eq!(program_of("/opt/homebrew/bin/node"), "node");
        assert_eq!(program_of(r"C:\Program Files\ChatGPT\ChatGPT.exe"), "ChatGPT.exe");
        assert_eq!(program_of(r"C:\Windows\explorer.exe (user)"), "explorer.exe");
        assert_eq!(program_of("/Applications/Foo (Beta).app/Contents/MacOS/Foo"), "Foo (Beta).app");
        assert_eq!(program_of(""), "");
    }

    #[test]
    fn names_compare_across_systems() {
        assert_eq!(name_key("XRAY.EXE"), name_key("xray"));
        assert!(is_bundle("Cursor.app") && is_bundle("cursor.APP") && !is_bundle(".app") && !is_bundle("Code.exe"));
        assert_eq!(display_name("Visual Studio Code.app"), "Visual Studio Code");
        assert_eq!(display_name("Code.EXE"), "Code");
        assert_eq!(display_name("node"), "node");
    }

    #[test]
    fn rules_follow_the_system() {
        let names = strings(&["xray.exe", "Xray", "Cursor.app", "node", "Visual Studio Code.app"]);
        assert_eq!(process_names(&names, Os::Windows), strings(&["xray.exe", "node"]));
        assert_eq!(process_names(&names, Os::MacOs), strings(&["xray", "node"]));
        assert!(bundle_patterns(&names, Os::Windows).is_empty());
        assert_eq!(
            bundle_patterns(&names, Os::MacOs),
            strings(&[r"(?i)/Cursor\.app/Contents/", r"(?i)/Visual Studio Code\.app/Contents/"])
        );
        assert_eq!(regex_escape("a+b (c).app"), r"a\+b \(c\)\.app");
    }

    #[test]
    fn system_programs_are_hidden_on_macos_only() {
        assert!(!is_user_program("/System/Library/CoreServices/Finder.app/Contents/MacOS/Finder", Os::MacOs));
        assert!(!is_user_program("/usr/libexec/trustd", Os::MacOs));
        assert!(is_user_program("/usr/local/bin/codex", Os::MacOs));
        assert!(is_user_program("/Applications/Cursor.app/Contents/MacOS/Cursor", Os::MacOs));
        assert!(is_user_program("/System/anything", Os::Windows));
    }
}
