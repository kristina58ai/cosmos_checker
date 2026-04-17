//! Парсер 4 форматов прокси-строк:
//! 1. `host:port` — HTTP без авторизации.
//! 2. `host:port:user:pass` — HTTP с авторизацией (традиционный список).
//! 3. `scheme://host:port` — URL-форма (http/https/socks5).
//! 4. `scheme://user:pass@host:port` — URL-форма с авторизацией.
//!
//! Комментарии в файле: строка начинающаяся с `#` или `//` — игнорируется.
//! Пустые строки пропускаются. Дубликаты (по [`Proxy::id`]) удаляются.

use std::collections::HashSet;
use std::path::Path;

use super::types::{Proxy, ProxyError, Scheme};

/// Парсит одну строку. Возвращает `None`, если строка — комментарий или пустая.
pub fn parse_line(raw: &str) -> Result<Option<Proxy>, ProxyError> {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
        return Ok(None);
    }

    // URL-форма: есть "://"
    if let Some(idx) = line.find("://") {
        let scheme_s = &line[..idx];
        let rest = &line[idx + 3..];
        let scheme = Scheme::parse(scheme_s)
            .ok_or_else(|| ProxyError::Invalid(format!("unknown scheme `{scheme_s}`")))?;
        return parse_url_rest(scheme, rest).map(Some);
    }

    // Колон-форма: host:port или host:port:user:pass.
    let parts: Vec<&str> = line.split(':').collect();
    match parts.len() {
        2 => {
            let (host, port) = parse_host_port(parts[0], parts[1])?;
            Ok(Some(Proxy {
                scheme: Scheme::Http,
                host,
                port,
                username: None,
                password: None,
            }))
        }
        4 => {
            let (host, port) = parse_host_port(parts[0], parts[1])?;
            let user = parts[2].to_owned();
            let pass = parts[3].to_owned();
            if user.is_empty() {
                return Err(ProxyError::Invalid(format!("empty username in `{line}`")));
            }
            Ok(Some(Proxy {
                scheme: Scheme::Http,
                host,
                port,
                username: Some(user),
                password: Some(pass),
            }))
        }
        _ => Err(ProxyError::Invalid(format!(
            "unrecognized proxy format `{line}`"
        ))),
    }
}

fn parse_url_rest(scheme: Scheme, rest: &str) -> Result<Proxy, ProxyError> {
    // rest: [user:pass@]host:port[/...trailing...]
    // Отбрасываем всё после первого '/'.
    let rest = rest.split('/').next().unwrap_or(rest);
    if rest.is_empty() {
        return Err(ProxyError::Invalid("empty authority".into()));
    }

    let (userinfo, hostport) = match rest.rsplit_once('@') {
        Some((u, h)) => (Some(u), h),
        None => (None, rest),
    };

    let (host, port_s) = hostport
        .rsplit_once(':')
        .ok_or_else(|| ProxyError::Invalid(format!("missing port in `{hostport}`")))?;
    let (host, port) = parse_host_port(host, port_s)?;

    let (username, password) = match userinfo {
        None => (None, None),
        Some(ui) => match ui.split_once(':') {
            Some((u, p)) => {
                if u.is_empty() {
                    return Err(ProxyError::Invalid("empty username in URL".into()));
                }
                (Some(u.to_owned()), Some(p.to_owned()))
            }
            None => {
                if ui.is_empty() {
                    return Err(ProxyError::Invalid("empty userinfo".into()));
                }
                (Some(ui.to_owned()), None)
            }
        },
    };

    Ok(Proxy {
        scheme,
        host,
        port,
        username,
        password,
    })
}

fn parse_host_port(host: &str, port: &str) -> Result<(String, u16), ProxyError> {
    if host.is_empty() {
        return Err(ProxyError::Invalid("empty host".into()));
    }
    let port: u16 = port
        .parse()
        .map_err(|_| ProxyError::Invalid(format!("bad port `{port}`")))?;
    if port == 0 {
        return Err(ProxyError::Invalid("port 0".into()));
    }
    Ok((host.to_owned(), port))
}

/// Парсит произвольный текст как список прокси (по строке).
/// Невалидные строки собираются в `errors`, дубликаты (по [`Proxy::id`]) — удаляются.
pub fn parse_text(text: &str) -> (Vec<Proxy>, Vec<String>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for (ln, raw) in text.lines().enumerate() {
        match parse_line(raw) {
            Ok(None) => {}
            Ok(Some(p)) => {
                let id = p.id();
                if seen.insert(id) {
                    out.push(p);
                }
            }
            Err(e) => errors.push(format!("line {}: {e}", ln + 1)),
        }
    }
    (out, errors)
}

/// Читает файл со списком прокси и парсит его.
pub fn parse_file(path: impl AsRef<Path>) -> Result<(Vec<Proxy>, Vec<String>), ProxyError> {
    let text = std::fs::read_to_string(path)?;
    Ok(parse_text(&text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_host_port_http() {
        let p = parse_line("1.2.3.4:8080").unwrap().unwrap();
        assert_eq!(p.scheme, Scheme::Http);
        assert_eq!(p.host, "1.2.3.4");
        assert_eq!(p.port, 8080);
        assert!(p.username.is_none());
    }

    #[test]
    fn fmt_host_port_user_pass() {
        let p = parse_line("1.2.3.4:8080:alice:secret").unwrap().unwrap();
        assert_eq!(p.scheme, Scheme::Http);
        assert_eq!(p.username.as_deref(), Some("alice"));
        assert_eq!(p.password.as_deref(), Some("secret"));
    }

    #[test]
    fn fmt_url_socks5_with_auth() {
        let p = parse_line("socks5://u:p@h.example:1080").unwrap().unwrap();
        assert_eq!(p.scheme, Scheme::Socks5);
        assert_eq!(p.host, "h.example");
        assert_eq!(p.port, 1080);
        assert_eq!(p.username.as_deref(), Some("u"));
        assert_eq!(p.password.as_deref(), Some("p"));
    }

    #[test]
    fn fmt_url_http_no_auth() {
        let p = parse_line("http://1.2.3.4:80").unwrap().unwrap();
        assert_eq!(p.scheme, Scheme::Http);
        assert!(p.username.is_none());
    }

    #[test]
    fn comments_and_empty_skipped() {
        assert!(parse_line("").unwrap().is_none());
        assert!(parse_line("   ").unwrap().is_none());
        assert!(parse_line("# comment").unwrap().is_none());
        assert!(parse_line("// also comment").unwrap().is_none());
    }

    #[test]
    fn invalid_formats_error() {
        assert!(parse_line("not_a_proxy").is_err());
        assert!(parse_line("1.2.3.4").is_err());
        assert!(parse_line("1.2.3.4:notaport").is_err());
        assert!(parse_line("1.2.3.4:0").is_err());
        assert!(parse_line("ftp://1.2.3.4:21").is_err());
        assert!(parse_line(":8080").is_err());
    }

    #[test]
    fn parse_text_deduplicates() {
        let text = "\
1.2.3.4:8080
# comment
1.2.3.4:8080
http://1.2.3.4:8080
socks5://u:p@h:1080
socks5://u:p@h:1080
";
        let (list, errs) = parse_text(text);
        assert!(errs.is_empty());
        // 1.2.3.4:8080 (http no auth) и http://1.2.3.4:8080 дают одинаковый id.
        // socks5 дедуплицируется.
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn parse_text_collects_errors() {
        let text = "1.2.3.4:80\ngarbage\n5.6.7.8:9090";
        let (list, errs) = parse_text(text);
        assert_eq!(list.len(), 2);
        assert_eq!(errs.len(), 1);
        assert!(errs[0].contains("line 2"));
    }

    #[test]
    fn parse_text_empty_input() {
        let (list, errs) = parse_text("");
        assert!(list.is_empty());
        assert!(errs.is_empty());
    }

    #[test]
    fn mixed_formats_all_parsed() {
        let text = "\
1.2.3.4:8080
5.6.7.8:9090:u:p
http://10.0.0.1:3128
socks5://u:pw@h.example:1080
";
        let (list, errs) = parse_text(text);
        assert!(errs.is_empty());
        assert_eq!(list.len(), 4);
        assert_eq!(list[0].scheme, Scheme::Http);
        assert!(list[1].username.is_some());
        assert_eq!(list[2].host, "10.0.0.1");
        assert_eq!(list[3].scheme, Scheme::Socks5);
    }
}
