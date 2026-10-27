//! Turning whatever the user pasted into a domain suffix sing-box can match.

use std::net::IpAddr;

/// `https://www.Example.com:8443/a?b` → `example.com`; `例子.测试` → punycode.
///
/// Matching is by suffix, so the result covers every subdomain. A leading
/// `www.` is dropped because people paste the address bar, and keeping it
/// would leave the bare domain and its other subdomains out.
pub fn normalise_domain(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("请输入网址或域名".into());
    }
    // "*.example.com" and ".example.com" are common ways to write "the whole domain".
    let trimmed = trimmed.trim_start_matches("*.").trim_start_matches('.');
    let with_scheme = if trimmed.contains("://") { trimmed.to_owned() } else { format!("http://{trimmed}") };
    let parsed = url::Url::parse(&with_scheme).map_err(|_| format!("“{}”不是有效的网址或域名", input.trim()))?;
    let host = match parsed.host() {
        Some(url::Host::Domain(domain)) => domain.to_owned(),
        Some(url::Host::Ipv4(_)) | Some(url::Host::Ipv6(_)) => {
            return Err("这里只能填写域名；IP 地址请通过“添加应用”按程序分流".into());
        }
        None => return Err(format!("“{}”里没有找到域名", input.trim())),
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.parse::<IpAddr>().is_ok() {
        return Err("这里只能填写域名；IP 地址请通过“添加应用”按程序分流".into());
    }
    let host = host.strip_prefix("www.").filter(|rest| rest.contains('.')).unwrap_or(&host).to_owned();
    validate(&host)?;
    Ok(host)
}

fn validate(host: &str) -> Result<(), String> {
    let invalid = || Err(format!("“{host}”不是有效的域名"));
    if host.is_empty() || host.len() > 253 {
        return invalid();
    }
    for label in host.split('.') {
        let ok_chars = label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if label.is_empty() || label.len() > 63 || !ok_chars || label.starts_with('-') || label.ends_with('-') {
            return invalid();
        }
    }
    Ok(())
}

/// Normalise a list, dropping entries that fail and duplicates. Used for lists
/// read from settings, where one bad entry must not discard the rest.
pub fn normalise_all<'a>(items: impl IntoIterator<Item = &'a String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        if let Ok(domain) = normalise_domain(item)
            && !out.contains(&domain) {
                out.push(domain);
            }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_scheme_path_port_and_www() {
        assert_eq!(normalise_domain("https://www.Example.com:8443/a?b=1").unwrap(), "example.com");
        assert_eq!(normalise_domain("chat.openai.com/c/123").unwrap(), "chat.openai.com");
        assert_eq!(normalise_domain("  github.com  ").unwrap(), "github.com");
        assert_eq!(normalise_domain("*.corp.example.cn").unwrap(), "corp.example.cn");
        assert_eq!(normalise_domain("example.com.").unwrap(), "example.com");
    }

    #[test]
    fn keeps_www_when_it_is_the_whole_domain() {
        assert_eq!(normalise_domain("www.com").unwrap(), "www.com");
    }

    #[test]
    fn converts_international_names_to_punycode() {
        assert_eq!(normalise_domain("例子.测试").unwrap(), "xn--fsqu00a.xn--0zwm56d");
    }

    #[test]
    fn single_label_suffixes_are_allowed_for_intranet_lists() {
        assert_eq!(normalise_domain("corp").unwrap(), "corp");
    }

    #[test]
    fn rejects_ips_and_garbage() {
        assert!(normalise_domain("1.1.1.1").is_err());
        assert!(normalise_domain("http://[::1]/").is_err());
        assert!(normalise_domain("").is_err());
        assert!(normalise_domain("exa mple.com").is_err());
        assert!(normalise_domain("-bad-.com").is_err());
    }

    #[test]
    fn normalise_all_skips_bad_entries_and_duplicates() {
        let items = vec!["a.com".to_owned(), "1.2.3.4".to_owned(), "https://A.com/x".to_owned(), "b.cn".to_owned()];
        assert_eq!(normalise_all(&items), vec!["a.com", "b.cn"]);
    }
}
