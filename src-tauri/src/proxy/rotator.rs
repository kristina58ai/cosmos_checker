//! Round-robin ротатор прокси с health-tracking'ом.
//!
//! Потокобезопасен (Arc<Mutex<..>> + AtomicUsize). Стратегия такая же, как
//! в `transport::EndpointPool`, но state хранит `Proxy` (не String).
//!
//! Семантика:
//! - `next()` — возвращает следующий *здоровый* прокси по круговой очереди;
//!   если все помечены unhealthy — `None`.
//! - `mark_bad` / `mark_good` — по `Proxy::id()`.
//! - Пустой пул всегда возвращает `None` (работа "без прокси").

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::types::Proxy;

#[derive(Debug, Clone)]
struct Slot {
    proxy: Proxy,
    healthy: bool,
}

#[derive(Debug, Default)]
struct Inner {
    slots: Mutex<Vec<Slot>>,
    cursor: AtomicUsize,
}

/// Пул прокси с round-robin ротацией.
#[derive(Clone, Default)]
pub struct ProxyPool {
    inner: Arc<Inner>,
}

impl ProxyPool {
    pub fn new(proxies: Vec<Proxy>) -> Self {
        let slots = proxies
            .into_iter()
            .map(|p| Slot {
                proxy: p,
                healthy: true,
            })
            .collect();
        Self {
            inner: Arc::new(Inner {
                slots: Mutex::new(slots),
                cursor: AtomicUsize::new(0),
            }),
        }
    }

    pub fn len(&self) -> usize {
        self.inner.slots.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn healthy_count(&self) -> usize {
        self.inner
            .slots
            .lock()
            .unwrap()
            .iter()
            .filter(|s| s.healthy)
            .count()
    }

    /// Следующий здоровый прокси или `None`.
    pub fn next(&self) -> Option<Proxy> {
        let slots = self.inner.slots.lock().unwrap();
        let n = slots.len();
        if n == 0 {
            return None;
        }
        for _ in 0..n {
            let i = self.inner.cursor.fetch_add(1, Ordering::Relaxed) % n;
            let s = &slots[i];
            if s.healthy {
                return Some(s.proxy.clone());
            }
        }
        None
    }

    pub fn mark_bad(&self, id: &str) {
        let mut slots = self.inner.slots.lock().unwrap();
        if let Some(s) = slots.iter_mut().find(|s| s.proxy.id() == id) {
            s.healthy = false;
        }
    }

    pub fn mark_good(&self, id: &str) {
        let mut slots = self.inner.slots.lock().unwrap();
        if let Some(s) = slots.iter_mut().find(|s| s.proxy.id() == id) {
            s.healthy = true;
        }
    }

    /// Возвращает все прокси (healthy + unhealthy) — для UI/диагностики.
    pub fn snapshot(&self) -> Vec<Proxy> {
        self.inner
            .slots
            .lock()
            .unwrap()
            .iter()
            .map(|s| s.proxy.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::Scheme;
    use super::*;

    fn proxy(host: &str) -> Proxy {
        Proxy {
            scheme: Scheme::Http,
            host: host.into(),
            port: 8080,
            username: None,
            password: None,
        }
    }

    #[test]
    fn empty_pool_returns_none() {
        let pool = ProxyPool::new(vec![]);
        assert!(pool.is_empty());
        assert!(pool.next().is_none());
    }

    #[test]
    fn round_robin_rotation() {
        let pool = ProxyPool::new(vec![proxy("a"), proxy("b"), proxy("c")]);
        let seq: Vec<_> = (0..6).filter_map(|_| pool.next()).map(|p| p.host).collect();
        // Порядок зависит от стартовой позиции cursor'а, но каждый хост встречается по 2 раза.
        let mut counts = std::collections::HashMap::new();
        for h in &seq {
            *counts.entry(h.clone()).or_insert(0) += 1;
        }
        assert_eq!(counts.len(), 3);
        for v in counts.values() {
            assert_eq!(*v, 2);
        }
    }

    #[test]
    fn mark_bad_skips_proxy() {
        let a = proxy("a");
        let b = proxy("b");
        let pool = ProxyPool::new(vec![a.clone(), b.clone()]);
        pool.mark_bad(&a.id());
        assert_eq!(pool.healthy_count(), 1);
        for _ in 0..5 {
            let got = pool.next().unwrap();
            assert_eq!(got.host, "b");
        }
    }

    #[test]
    fn all_unhealthy_returns_none() {
        let a = proxy("a");
        let pool = ProxyPool::new(vec![a.clone()]);
        pool.mark_bad(&a.id());
        assert!(pool.next().is_none());
    }

    #[test]
    fn mark_good_restores() {
        let a = proxy("a");
        let pool = ProxyPool::new(vec![a.clone()]);
        pool.mark_bad(&a.id());
        assert!(pool.next().is_none());
        pool.mark_good(&a.id());
        assert!(pool.next().is_some());
    }
}
