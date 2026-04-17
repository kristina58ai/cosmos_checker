//! Пул endpoint'ов с round-robin ротацией и пометкой unhealthy.
//!
//! Thread-safe: внутри `Mutex<Vec<EndpointState>>` + `AtomicUsize` для
//! счётчика. Дёшево клонируется (`Arc` внутри).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Состояние одного endpoint'а в пуле.
#[derive(Debug, Clone)]
struct EndpointState {
    url: String,
    healthy: bool,
}

/// Pool endpoint'ов одного типа (например, все REST URLs одной сети).
#[derive(Clone)]
pub struct EndpointPool {
    inner: Arc<PoolInner>,
}

struct PoolInner {
    endpoints: Mutex<Vec<EndpointState>>,
    cursor: AtomicUsize,
}

impl EndpointPool {
    pub fn new<I: IntoIterator<Item = String>>(urls: I) -> Self {
        let endpoints = urls
            .into_iter()
            .map(|url| EndpointState { url, healthy: true })
            .collect::<Vec<_>>();
        Self {
            inner: Arc::new(PoolInner {
                endpoints: Mutex::new(endpoints),
                cursor: AtomicUsize::new(0),
            }),
        }
    }

    pub fn len(&self) -> usize {
        self.inner.endpoints.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Вернуть следующий healthy endpoint и сдвинуть курсор.
    /// `None` если все endpoint'ы unhealthy или пул пуст.
    pub fn next(&self) -> Option<String> {
        let eps = self.inner.endpoints.lock().unwrap();
        let n = eps.len();
        if n == 0 {
            return None;
        }
        // Пытаемся N раз — если все unhealthy, вернём None.
        for _ in 0..n {
            let idx = self.inner.cursor.fetch_add(1, Ordering::Relaxed) % n;
            if eps[idx].healthy {
                return Some(eps[idx].url.clone());
            }
        }
        None
    }

    /// Снимок всех URL (для тестов / диагностики).
    pub fn urls(&self) -> Vec<String> {
        self.inner
            .endpoints
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.url.clone())
            .collect()
    }

    /// Пометить endpoint как unhealthy. Идемпотентно.
    pub fn mark_unhealthy(&self, url: &str) {
        let mut eps = self.inner.endpoints.lock().unwrap();
        if let Some(e) = eps.iter_mut().find(|e| e.url == url) {
            e.healthy = false;
        }
    }

    pub fn mark_healthy(&self, url: &str) {
        let mut eps = self.inner.endpoints.lock().unwrap();
        if let Some(e) = eps.iter_mut().find(|e| e.url == url) {
            e.healthy = true;
        }
    }

    pub fn healthy_count(&self) -> usize {
        self.inner
            .endpoints
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.healthy)
            .count()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_robin_rotation() {
        let pool = EndpointPool::new(vec!["a".into(), "b".into(), "c".into()]);
        let seq: Vec<_> = (0..6).map(|_| pool.next().unwrap()).collect();
        assert_eq!(seq, vec!["a", "b", "c", "a", "b", "c"]);
    }

    #[test]
    fn empty_pool_returns_none() {
        let pool: EndpointPool = EndpointPool::new(Vec::<String>::new());
        assert!(pool.is_empty());
        assert_eq!(pool.next(), None);
    }

    #[test]
    fn unhealthy_endpoints_skipped() {
        let pool = EndpointPool::new(vec!["a".into(), "b".into(), "c".into()]);
        pool.mark_unhealthy("b");
        let seq: Vec<_> = (0..4).map(|_| pool.next().unwrap()).collect();
        // Ожидаем чередование a / c (b пропускается).
        assert!(seq.iter().all(|s| s == "a" || s == "c"));
        assert!(seq.contains(&"a".to_string()));
        assert!(seq.contains(&"c".to_string()));
    }

    #[test]
    fn all_unhealthy_returns_none() {
        let pool = EndpointPool::new(vec!["a".into(), "b".into()]);
        pool.mark_unhealthy("a");
        pool.mark_unhealthy("b");
        assert_eq!(pool.next(), None);
        assert_eq!(pool.healthy_count(), 0);
    }

    #[test]
    fn mark_healthy_restores() {
        let pool = EndpointPool::new(vec!["a".into()]);
        pool.mark_unhealthy("a");
        assert_eq!(pool.next(), None);
        pool.mark_healthy("a");
        assert_eq!(pool.next(), Some("a".to_string()));
    }
}
