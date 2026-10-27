//! Whether only administrators can change a file or folder.
//!
//! ProxyPilot runs elevated and starts sing-box elevated, so whatever it
//! executes or hands to sing-box must be out of reach of ordinary programs.
//! Folders outside Program Files often are not: on a second drive, signed-in
//! users can usually modify anything created there. We read the owner and
//! DACL as SDDL and answer conservatively: anything we cannot read counts as
//! unsafe.

use crate::sys;
use std::path::Path;

/// Rights that let the holder change, replace or delete the object or rewrite
/// its permissions: write and append data (add file and add folder on
/// folders), delete child, DELETE, WRITE_DAC, WRITE_OWNER, and the generic
/// and maximum-allowed rights that include them.
const TAMPER: u32 = 0x0000_0002 | 0x0000_0004 | 0x0000_0040 | 0x0001_0000 | 0x0004_0000 | 0x0008_0000
    | 0x0200_0000 | 0x1000_0000 | 0x4000_0000;

/// Two-letter SDDL right codes. Directory-service codes are listed by value
/// because Windows prints file rights with them too (`DC` is write data).
const RIGHTS: &[(&str, u32)] = &[
    ("GA", 0x1000_0000), ("GR", 0x8000_0000), ("GW", 0x4000_0000), ("GX", 0x2000_0000),
    ("RC", 0x0002_0000), ("SD", 0x0001_0000), ("WD", 0x0004_0000), ("WO", 0x0008_0000),
    ("CC", 0x1), ("DC", 0x2), ("LC", 0x4), ("SW", 0x8), ("RP", 0x10), ("WP", 0x20), ("DT", 0x40), ("LO", 0x80),
    ("CR", 0x100), ("FA", 0x001F_01FF), ("FR", 0x0012_0089), ("FW", 0x0012_0116), ("FX", 0x0012_00A0),
    ("KA", 0x000F_003F), ("KR", 0x0002_0019), ("KW", 0x0002_0006), ("KX", 0x0002_0019),
    ("NR", 0x1), ("NW", 0x2), ("NX", 0x4),
];

const TRUSTED_INSTALLER: &str = "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464";

/// Whether only administrators can change `path`. False when its permissions
/// cannot be read, and on platforms without a check yet.
pub fn admin_only(path: &Path) -> bool {
    sys::security_descriptor(path).is_ok_and(|sddl| admin_only_sddl(&sddl))
}

/// The same for an owner and DACL in SDDL, as Windows formats them.
pub fn admin_only_sddl(sddl: &str) -> bool {
    sddl.is_ascii() && parse(sddl).unwrap_or(false)
}

/// SYSTEM, the Administrators group and TrustedInstaller.
fn trusted(sid: &str) -> bool {
    matches!(sid, "SY" | "BA" | "S-1-5-18" | "S-1-5-32-544" | TRUSTED_INSTALLER)
}

/// SIDs no token holds: CREATOR OWNER and CREATOR GROUP are replaced on
/// inheritance, and OWNER RIGHTS is whoever owns it, which is checked first.
fn stand_in(sid: &str) -> bool {
    matches!(sid, "CO" | "CG" | "OW" | "S-1-3-0" | "S-1-3-1" | "S-1-3-4")
}

fn parse(sddl: &str) -> Option<bool> {
    // Each part is a letter and a colon; owner and group SIDs contain neither.
    let rest = sddl.strip_prefix("O:")?;
    let (owner, rest) = rest.split_at(rest.find(':')?.checked_sub(1)?);
    // An owner can always rewrite the DACL.
    if !trusted(owner) {
        return Some(false);
    }
    let rest = match rest.strip_prefix("G:") {
        Some(group) => &group[group.find(':')?.checked_sub(1)?..],
        None => rest,
    };
    dacl_safe(rest.strip_prefix("D:")?)
}

fn dacl_safe(dacl: &str) -> Option<bool> {
    // A null DACL lets everyone in.
    if dacl.starts_with("NO_ACCESS_CONTROL") {
        return Some(false);
    }
    let mut rest = dacl.trim_start_matches(['P', 'A', 'I', 'R']);
    while let Some(body) = rest.strip_prefix('(') {
        let end = closing(body)?;
        if !ace_safe(&body[..end])? {
            return Some(false);
        }
        rest = &body[end + 1..];
    }
    // Only a SACL may follow, and we did not ask for one.
    (rest.is_empty() || rest.starts_with("S:")).then_some(true)
}

/// Index of the `)` that ends an ACE; conditional ACEs nest parentheses.
fn closing(body: &str) -> Option<usize> {
    let (mut depth, mut quoted) = (0usize, false);
    for (i, c) in body.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted && depth == 0 => return Some(i),
            ')' if !quoted => depth -= 1,
            _ => {}
        }
    }
    None
}

/// type;flags;rights;object;inherited object;sid[;condition]
fn ace_safe(ace: &str) -> Option<bool> {
    let fields: Vec<&str> = ace.splitn(7, ';').collect();
    let [kind, flags, mask, _, _, sid, ..] = fields.as_slice() else { return None };
    let grants = match *kind {
        "A" | "OA" | "XA" | "ZA" => true,
        "D" | "OD" | "XD" | "AU" | "AL" | "OU" | "OL" | "ML" | "SP" | "XU" | "RA" | "SC" => false,
        _ => return None,
    };
    if !grants || pairs(flags)?.any(|flag| flag == "IO") {
        return Some(true);
    }
    let sid = sid.trim();
    Some(rights(mask)? & TAMPER == 0 || trusted(sid) || stand_in(sid))
}

fn rights(text: &str) -> Option<u32> {
    if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return u32::from_str_radix(hex, 16).ok();
    }
    if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
        return text.parse().ok();
    }
    pairs(text)?.try_fold(0, |mask, code| Some(mask | RIGHTS.iter().find(|(name, _)| *name == code)?.1))
}

/// SDDL flag and right strings are runs of two-letter codes.
fn pairs(text: &str) -> Option<impl Iterator<Item = &str>> {
    text.len().is_multiple_of(2).then(|| (0..text.len()).step_by(2).map(move |i| &text[i..i + 2]))
}

#[cfg(test)]
mod tests {
    use super::admin_only_sddl as safe;

    const USER: &str = "S-1-5-21-71959975-1839122372-1696552518-1001";

    #[test]
    fn program_files_is_admin_only() {
        let ti = super::TRUSTED_INSTALLER;
        let sddl = format!(
            "O:{ti}G:{ti}D:PAI(A;OICIIO;GA;;;CO)(A;OICIIO;GA;;;SY)(A;;0x1301bf;;;SY)(A;OICIIO;GA;;;BA)(A;;0x1301bf;;;BA)\
             (A;OICIIO;GXGR;;;BU)(A;;0x1200a9;;;BU)(A;CIIO;GA;;;{ti})(A;;FA;;;{ti})(A;;0x1200a9;;;AC)(A;OICIIO;GXGR;;;AC)\
             (A;;0x1200a9;;;S-1-15-2-2)(A;OICIIO;GXGR;;;S-1-15-2-2)"
        );
        assert!(safe(&sddl));
    }

    #[test]
    fn folder_locked_by_the_installer_is_admin_only() {
        assert!(safe(&format!("O:BAG:{USER}D:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)")));
        assert!(safe(&format!("O:BAG:{USER}D:AI(A;ID;FA;;;SY)(A;ID;FA;;;BA)(A;ID;0x1200a9;;;BU)")));
        assert!(safe("O:SYD:P(A;;FA;;;SY)"));
    }

    #[test]
    fn second_drive_folder_is_not() {
        // D:\ProxyPilot as installed before the fix: signed-in users may modify.
        let sddl = format!(
            "O:BAG:{USER}D:(A;ID;FA;;;BA)(A;OICIIOID;GA;;;BA)(A;ID;FA;;;SY)(A;OICIIOID;GA;;;SY)(A;ID;0x1301bf;;;AU)\
             (A;OICIIOID;SDGXGWGR;;;AU)(A;ID;0x1200a9;;;BU)(A;OICIIOID;GXGR;;;BU)"
        );
        assert!(!safe(&sddl));
        assert!(!safe(&format!("O:BAG:{USER}D:(A;ID;FA;;;BA)(A;ID;FA;;;SY)(A;ID;0x1301bf;;;AU)(A;ID;0x1200a9;;;BU)")));
    }

    #[test]
    fn any_tamper_right_for_others_is_unsafe() {
        for rights in ["FA", "FW", "GA", "GW", "WD", "WO", "SD", "DC", "0x2", "0x40", "0X10000", "2"] {
            assert!(!safe(&format!("O:BAD:P(A;;FA;;;SY)(A;;{rights};;;BU)")), "{rights}");
        }
        assert!(safe("O:BAD:P(A;;FA;;;SY)(A;;FRFX;;;BU)(A;;0x1200a9;;;WD)"));
    }

    #[test]
    fn owner_counts_because_it_can_rewrite_the_dacl() {
        assert!(!safe(&format!("O:{USER}G:{USER}D:P(A;;FA;;;SY)(A;;FA;;;BA)")));
        assert!(!safe("O:DAG:DAD:P(A;;FA;;;SY)"));
        assert!(safe("O:BAD:P(A;;FA;;;OW)(A;;FA;;;CO)(D;;FA;;;WD)"));
    }

    #[test]
    fn inherit_only_entries_do_not_apply_to_the_object() {
        assert!(safe("O:BAD:P(A;;FA;;;BA)(A;OICIIO;GA;;;BU)"));
        assert!(!safe("O:BAD:P(A;;FA;;;BA)(A;OICI;GA;;;BU)"));
    }

    #[test]
    fn conditional_entries_are_parsed_and_counted() {
        assert!(!safe(r#"O:BAD:P(A;;FA;;;BA)(XA;;FA;;;BU;(@User.Title == "a)(b"))"#));
        assert!(safe(r#"O:BAD:P(A;;FA;;;BA)(XA;;FX;;;BU;((Member_of {SID(BA)})))"#));
    }

    #[cfg(windows)]
    #[test]
    fn reads_real_folders() {
        let program_files = std::env::var_os("ProgramFiles").expect("ProgramFiles");
        assert!(super::admin_only(std::path::Path::new(&program_files)));
        assert!(!super::admin_only(&std::env::temp_dir()));
        assert!(!super::admin_only(std::path::Path::new(r"C:\no\such\folder")));
    }

    #[test]
    fn anything_unreadable_is_unsafe() {
        for sddl in [
            "",
            "O:BA",
            "O:BAD:NO_ACCESS_CONTROL",
            "O:BAD:P(A;;FA;;;BA",
            "O:BAD:P(A;;FA;;)",
            "O:BAD:P(Q;;FA;;;BU)",
            "O:BAD:P(A;;ZZ;;;BU)",
            "O:BAD:P(A;;FA;;;BA)junk",
            "O:BAD:P(A;;F;;;BU)",
            "O:BA\u{e9}D:P(A;;FA;;;BA)",
        ] {
            assert!(!safe(sddl), "{sddl}");
        }
    }
}
