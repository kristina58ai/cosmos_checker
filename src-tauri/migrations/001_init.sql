-- Cosmos Checker — SQLite Schema
-- Используется для кеширования и хранения результатов.
-- НИКОГДА не хранить seed-фразы и приватные ключи.

-- ============================================================
-- 1. Кеш Chain Registry
-- ============================================================

CREATE TABLE IF NOT EXISTS chains (
    chain_id        TEXT PRIMARY KEY,           -- "cosmoshub-4"
    chain_name      TEXT NOT NULL,              -- "cosmoshub"
    bech32_prefix   TEXT NOT NULL,              -- "cosmos"
    slip44          INTEGER NOT NULL DEFAULT 118, -- coin_type для HD derivation
    display_name    TEXT,                       -- "Cosmos Hub"
    logo_url        TEXT,                       -- URL иконки сети
    updated_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS chain_endpoints (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    chain_id        TEXT NOT NULL REFERENCES chains(chain_id) ON DELETE CASCADE,
    endpoint_type   TEXT NOT NULL CHECK(endpoint_type IN ('grpc', 'rest', 'rpc')),
    address         TEXT NOT NULL,              -- "https://grpc.cosmos.network:443"
    provider        TEXT,                       -- "Cosmos Network"
    is_healthy      INTEGER NOT NULL DEFAULT 1, -- 0 = unhealthy, 1 = healthy
    avg_latency_ms  INTEGER,                   -- средний ping
    last_check_at   TEXT,
    UNIQUE(chain_id, endpoint_type, address)
);

CREATE INDEX IF NOT EXISTS idx_endpoints_chain ON chain_endpoints(chain_id, endpoint_type);

CREATE TABLE IF NOT EXISTS chain_tokens (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    chain_id        TEXT NOT NULL REFERENCES chains(chain_id) ON DELETE CASCADE,
    denom           TEXT NOT NULL,              -- "uatom"
    display_denom   TEXT NOT NULL,              -- "ATOM"
    exponent        INTEGER NOT NULL DEFAULT 6, -- 10^exponent для конвертации
    UNIQUE(chain_id, denom)
);

-- ============================================================
-- 2. Сессии проверки
-- ============================================================

CREATE TABLE IF NOT EXISTS check_sessions (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    name            TEXT,                       -- опциональное название
    started_at      TEXT NOT NULL DEFAULT (datetime('now')),
    finished_at     TEXT,
    total_wallets   INTEGER NOT NULL DEFAULT 0,
    checked_wallets INTEGER NOT NULL DEFAULT 0,
    status          TEXT NOT NULL DEFAULT 'running'
                    CHECK(status IN ('running', 'completed', 'cancelled', 'error'))
);

-- ============================================================
-- 3. Результаты проверки
-- ============================================================

CREATE TABLE IF NOT EXISTS wallet_results (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id      INTEGER NOT NULL REFERENCES check_sessions(id) ON DELETE CASCADE,
    address         TEXT NOT NULL,              -- bech32 адрес
    chain_id        TEXT NOT NULL REFERENCES chains(chain_id),
    input_type      TEXT NOT NULL CHECK(input_type IN ('address', 'seed', 'private_key')),
    -- Балансы (хранятся в минимальных единицах, например uatom)
    balance_raw     TEXT,                       -- JSON: [{"denom":"uatom","amount":"1500000"}]
    balance_display TEXT,                       -- "1.5 ATOM"
    -- Стейкинг
    staked_raw      TEXT,                       -- JSON: delegations
    staked_display  TEXT,                       -- "10.0 ATOM"
    -- Rewards
    rewards_raw     TEXT,                       -- JSON: rewards
    rewards_display TEXT,                       -- "0.3 ATOM"
    -- Unbonding
    unbonding_raw   TEXT,                       -- JSON: unbonding
    unbonding_display TEXT,                     -- "0.0 ATOM"
    -- Метаданные
    has_funds       INTEGER NOT NULL DEFAULT 0, -- 1 если есть какие-либо средства
    error           TEXT,                       -- текст ошибки если запрос не удался
    checked_at      TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_results_session ON wallet_results(session_id);
CREATE INDEX IF NOT EXISTS idx_results_address ON wallet_results(address);
CREATE INDEX IF NOT EXISTS idx_results_has_funds ON wallet_results(session_id, has_funds);

-- ============================================================
-- 4. Настройки приложения
-- ============================================================

CREATE TABLE IF NOT EXISTS app_settings (
    key             TEXT PRIMARY KEY,
    value           TEXT NOT NULL
);

-- Дефолтные настройки
INSERT OR IGNORE INTO app_settings (key, value) VALUES
    ('max_concurrency', '100'),
    ('request_timeout_ms', '5000'),
    ('fallback_enabled', 'true'),
    ('chain_registry_cache_hours', '24'),
    ('proxy_rotation', 'round_robin');
