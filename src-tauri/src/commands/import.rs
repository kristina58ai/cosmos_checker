//! IPC: импорт wallet-файла и proxy-файла.
//!
//! `import_wallets` читает файл с путём, классифицирует строки
//! (см. [`crate::file_io::importer`]) и кладёт `Vec<InputEntry>` во
//! [`AppState::pending_imports`] под свежевыданный `import_token`. На
//! фронтенд возвращаются только числа и ошибки — ни seed, ни privkey не
//! пересекают IPC-границу.
//!
//! `import_proxies` читает файл и заполняет [`AppState::pending_proxies`].
//! Возвращается количество и список ошибок парсинга.

use serde::{Deserialize, Serialize};

use crate::file_io::import_file;
use crate::proxy::parse_file as parse_proxy_file;

use super::state::{AppState, CommandResult};

// ---------------------------------------------------------------------------
// Responses
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportWalletsResponse {
    /// Токен, по которому `start_check` заберёт записи.
    pub import_token: i64,
    pub total: usize,
    pub address: usize,
    pub seed12: usize,
    pub seed24: usize,
    pub private_key: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportProxiesResponse {
    pub total: usize,
    pub errors: Vec<String>,
}

// ---------------------------------------------------------------------------
// Inner
// ---------------------------------------------------------------------------

pub fn import_wallets_inner(state: &AppState, path: &str) -> CommandResult<ImportWalletsResponse> {
    let report = import_file(path)?;
    let (a, s12, s24, pk) = report.counts();
    let total = report.entries.len();

    let token = state.next_import_token();
    {
        let mut guard = state
            .pending_imports
            .lock()
            .expect("pending_imports mutex poisoned");
        guard.insert(token, report.entries);
    }

    Ok(ImportWalletsResponse {
        import_token: token,
        total,
        address: a,
        seed12: s12,
        seed24: s24,
        private_key: pk,
        errors: report.errors,
    })
}

pub fn import_proxies_inner(state: &AppState, path: &str) -> CommandResult<ImportProxiesResponse> {
    let (proxies, errors) = parse_proxy_file(path)?;
    let total = proxies.len();
    {
        let mut guard = state
            .pending_proxies
            .lock()
            .expect("pending_proxies mutex poisoned");
        *guard = proxies;
    }
    Ok(ImportProxiesResponse { total, errors })
}

// ---------------------------------------------------------------------------
// Tauri command wrappers
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn import_wallets(
    state: tauri::State<'_, AppState>,
    path: String,
) -> CommandResult<ImportWalletsResponse> {
    import_wallets_inner(state.inner(), &path)
}

#[tauri::command]
pub fn import_proxies(
    state: tauri::State<'_, AppState>,
    path: String,
) -> CommandResult<ImportProxiesResponse> {
    import_proxies_inner(state.inner(), &path)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn tmp_file(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    #[test]
    fn import_wallets_classifies_and_stores() {
        let st = AppState::new_in_memory().unwrap();
        // one address + one 12-word seed + one blank + one invalid
        let content = "\
cosmos19rl4cm2hmr8afy4kldpxz3fka4jguq0auqdal4
abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about

??? not valid ???
";
        let f = tmp_file(content);
        let resp = import_wallets_inner(&st, f.path().to_str().unwrap()).unwrap();
        assert_eq!(resp.address, 1);
        assert_eq!(resp.seed12, 1);
        assert_eq!(resp.total, 2);
        assert_eq!(resp.errors.len(), 1, "one invalid line expected");

        // Verify entries stashed under the returned token.
        let guard = st.pending_imports.lock().unwrap();
        let entries = guard.get(&resp.import_token).expect("stashed entries");
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn import_wallets_tokens_unique_per_call() {
        let st = AppState::new_in_memory().unwrap();
        let f = tmp_file("cosmos19rl4cm2hmr8afy4kldpxz3fka4jguq0auqdal4\n");
        let a = import_wallets_inner(&st, f.path().to_str().unwrap()).unwrap();
        let b = import_wallets_inner(&st, f.path().to_str().unwrap()).unwrap();
        assert_ne!(a.import_token, b.import_token);
    }

    #[test]
    fn import_proxies_reads_and_stores() {
        let st = AppState::new_in_memory().unwrap();
        let content = "\
1.2.3.4:8080
http://5.6.7.8:3128
not-a-proxy
";
        let f = tmp_file(content);
        let resp = import_proxies_inner(&st, f.path().to_str().unwrap()).unwrap();
        assert_eq!(resp.total, 2);
        assert_eq!(resp.errors.len(), 1);
        assert_eq!(st.pending_proxies.lock().unwrap().len(), 2);
    }

    #[test]
    fn response_serde_round_trip() {
        let r = ImportWalletsResponse {
            import_token: 42,
            total: 3,
            address: 2,
            seed12: 1,
            seed24: 0,
            private_key: 0,
            errors: vec!["line 2: invalid".into()],
        };
        let s = serde_json::to_string(&r).unwrap();
        let back: ImportWalletsResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(r, back);
    }
}
