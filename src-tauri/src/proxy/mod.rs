//! Proxy Manager — парсинг списков прокси, round-robin ротатор,
//! health-tracking. Stage 6.
//!
//! Поддерживаемые форматы (см. [`parser::parse_line`]):
//! - `host:port`
//! - `host:port:user:pass`
//! - `scheme://host:port`
//! - `scheme://user:pass@host:port`
//!
//! где scheme ∈ {http, https, socks5, socks5h}.
//!
//! Интеграция с `reqwest`: [`proxy_for_reqwest`] строит `reqwest::Proxy`
//! из нашего [`Proxy`] — используется в Stage 8 (checker pipeline), когда
//! per-request клиент привязывается к одному прокси из пула.

pub mod parser;
pub mod rotator;
pub mod types;

pub use parser::{parse_file, parse_line, parse_text};
pub use rotator::ProxyPool;
pub use types::{Proxy, ProxyError, Scheme};

/// Конструирует `reqwest::Proxy::all(...)` из нашего `Proxy`.
///
/// Нужен для per-request клиентов в checker-pipeline'е: под каждый батч
/// запросов берём один прокси из [`ProxyPool`] и строим `reqwest::Client`
/// с этим прокси.
pub fn proxy_for_reqwest(p: &Proxy) -> Result<reqwest::Proxy, reqwest::Error> {
    let url = p.to_url();
    let mut rp = reqwest::Proxy::all(&url)?;
    if let (Some(u), Some(pw)) = (&p.username, &p.password) {
        rp = rp.basic_auth(u, pw);
    }
    Ok(rp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reqwest_proxy_builds_for_all_schemes() {
        for raw in [
            "http://1.2.3.4:8080",
            "https://1.2.3.4:8443",
            "socks5://1.2.3.4:1080",
            "1.2.3.4:8080:u:p",
        ] {
            let p = parse_line(raw).unwrap().unwrap();
            let _ = proxy_for_reqwest(&p).expect("reqwest::Proxy builds");
        }
    }
}
