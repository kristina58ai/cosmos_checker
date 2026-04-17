//! Proxy types + ошибки.

use std::fmt;

use thiserror::Error;

/// Схема прокси.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scheme {
    Http,
    Https,
    Socks5,
}

impl Scheme {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scheme::Http => "http",
            Scheme::Https => "https",
            Scheme::Socks5 => "socks5",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "http" => Some(Scheme::Http),
            "https" => Some(Scheme::Https),
            "socks5" | "socks5h" => Some(Scheme::Socks5),
            _ => None,
        }
    }
}

/// Нормализованное описание прокси. Кредиты хранятся как обычный String —
/// логины/пароли прокси не являются криптографическими секретами (это не
/// seed-фразы), но в логах мы их всё равно маскируем (`Display` не печатает
/// пароль целиком).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Proxy {
    pub scheme: Scheme,
    pub host: String,
    pub port: u16,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl Proxy {
    /// URL в формате, который понимает `reqwest::Proxy::all(...)`.
    pub fn to_url(&self) -> String {
        match (&self.username, &self.password) {
            (Some(u), Some(p)) => format!(
                "{}://{}:{}@{}:{}",
                self.scheme.as_str(),
                urlencode(u),
                urlencode(p),
                self.host,
                self.port
            ),
            (Some(u), None) => format!(
                "{}://{}@{}:{}",
                self.scheme.as_str(),
                urlencode(u),
                self.host,
                self.port
            ),
            _ => format!("{}://{}:{}", self.scheme.as_str(), self.host, self.port),
        }
    }

    /// Стабильный идентификатор (scheme+host+port+user) — используется для
    /// дедупликации и health-tracking'а.
    pub fn id(&self) -> String {
        format!(
            "{}://{}@{}:{}",
            self.scheme.as_str(),
            self.username.as_deref().unwrap_or(""),
            self.host,
            self.port
        )
    }
}

impl fmt::Display for Proxy {
    /// Безопасное представление для логов — без пароля.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.username {
            Some(u) => write!(
                f,
                "{}://{}:***@{}:{}",
                self.scheme.as_str(),
                u,
                self.host,
                self.port
            ),
            None => write!(f, "{}://{}:{}", self.scheme.as_str(), self.host, self.port),
        }
    }
}

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("empty input")]
    Empty,

    #[error("invalid proxy line: {0}")]
    Invalid(String),

    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Минимальный percent-encoding для логина/пароля в URL.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_hides_password() {
        let p = Proxy {
            scheme: Scheme::Http,
            host: "1.2.3.4".into(),
            port: 8080,
            username: Some("user".into()),
            password: Some("SECRET".into()),
        };
        let s = format!("{p}");
        assert!(!s.contains("SECRET"));
        assert!(s.contains("user"));
        assert!(s.contains("***"));
    }

    #[test]
    fn to_url_with_auth() {
        let p = Proxy {
            scheme: Scheme::Socks5,
            host: "h".into(),
            port: 1080,
            username: Some("u".into()),
            password: Some("p@ss".into()),
        };
        assert_eq!(p.to_url(), "socks5://u:p%40ss@h:1080");
    }

    #[test]
    fn to_url_no_auth() {
        let p = Proxy {
            scheme: Scheme::Http,
            host: "1.2.3.4".into(),
            port: 80,
            username: None,
            password: None,
        };
        assert_eq!(p.to_url(), "http://1.2.3.4:80");
    }

    #[test]
    fn id_stable_across_clones() {
        let p = Proxy {
            scheme: Scheme::Http,
            host: "h".into(),
            port: 1,
            username: Some("u".into()),
            password: Some("p".into()),
        };
        assert_eq!(p.id(), p.clone().id());
    }
}
