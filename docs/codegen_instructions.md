# Cosmos Checker — Codegen Instructions for Claude Code

> Пошаговые инструкции для кодогенерации. Выполняй строго по порядку.

---

## Порядок генерации модулей

### Шаг 1: Tauri Project Scaffold (Модуль 1.1)

**Что делать:**
1. `npm create tauri-app@latest cosmos-checker -- --template react-ts`
2. Добавить в `Cargo.toml` зависимости:
   ```toml
   [dependencies]
   tokio = { version = "1", features = ["full"] }
   reqwest = { version = "0.12", features = ["json", "socks", "rustls-tls"] }
   tonic = "0.12"
   prost = "0.13"
   bip39 = "2"
   bip32 = "0.5"
   k256 = "0.13"
   bech32 = "0.11"
   zeroize = { version = "1", features = ["derive"] }
   secrecy = "0.10"
   rusqlite = { version = "0.31", features = ["bundled"] }
   serde = { version = "1", features = ["derive"] }
   serde_json = "1"
   tracing = "0.1"
   tracing-subscriber = "0.3"
   anyhow = "1"
   thiserror = "1"
   sha2 = "0.10"
   ripemd = "0.1"
   hex = "0.4"
   tokio-socks = "0.5"
   tauri = { version = "2", features = [] }
   ```
3. Настроить `tauri.conf.json`: минимальные permissions (fs:read, dialog:open, dialog:save)
4. Создать структуру директорий:
   ```
   src-tauri/src/
     ├── main.rs
     ├── commands/          # Tauri command handlers
     ├── crypto/            # Key Deriver
     ├── registry/          # Chain Registry Manager
     ├── transport/         # Cosmos API client (gRPC + REST)
     ├── proxy/             # Proxy Manager
     ├── io/                # File Importer + Result Exporter
     ├── checker/           # Checker Pipeline
     ├── db/                # SQLite layer
     └── models/            # Shared types
   ```
5. Добавить в React: `npm install zustand @tanstack/react-table tailwindcss`

**Критерий готовности:** `cargo tauri dev` открывает окно, invoke("greet") работает.
**Коммит:** `feat: scaffold Tauri v2 project with Rust + React`

---

### Шаг 2: Key Deriver (Модуль 1.2)

**Что делать:**
1. Создать `src-tauri/src/crypto/mod.rs` и `key_deriver.rs`
2. Реализовать:
   ```rust
   pub fn derive_address(input: &WalletInput, chain: &ChainConfig) -> Result<String>
   ```
3. Типы:
   ```rust
   enum WalletInput {
       Address(String),
       Seed(Secret<String>),       // secrecy::Secret
       PrivateKey(Secret<Vec<u8>>),
   }
   struct ChainConfig {
       bech32_prefix: String,
       slip44: u32,
   }
   ```
4. Алгоритм для Seed: BIP39 → BIP32 (m/44'/{slip44}'/0'/0/0) → secp256k1 pubkey → SHA256 → RIPEMD160 → Bech32
5. Алгоритм для PrivateKey: bytes → secp256k1 pubkey → SHA256 → RIPEMD160 → Bech32
6. Для Address: return as-is (passthrough)
7. Все Secret<T> поля используют `zeroize` при Drop
8. Написать тесты (8 штук из tests_spec.md)

**Критерий готовности:** `cargo test crypto` — все 8 тестов проходят.
**Коммит:** `feat: implement key deriver (BIP39/BIP32/secp256k1/Bech32)`

---

### Шаг 3: SQLite Layer (Модуль 2.2)

**Что делать:**
1. Создать `src-tauri/src/db/mod.rs` и `database.rs`
2. Реализовать init_db() — создаёт все таблицы из schema.sql
3. CRUD методы: insert_chain, get_chains, insert_result, get_results, get/set_settings
4. Все запросы через prepared statements (rusqlite params)
5. Написать тесты (5 штук, in-memory SQLite)

**Критерий готовности:** `cargo test db` — все 5 тестов проходят.
**Коммит:** `feat: implement SQLite database layer with schema`

---

### Шаг 4: Chain Registry Manager (Модуль 2.1)

**Что делать:**
1. Создать `src-tauri/src/registry/chain_registry.rs`
2. Загрузка chain-registry: HTTP GET к GitHub API или raw content
3. Парсинг JSON → Vec<ChainConfig> (chain_id, bech32_prefix, slip44, endpoints)
4. Кеширование в SQLite (таблицы chains, chain_endpoints, chain_tokens)
5. force_refresh: обновить кеш с GitHub
6. Написать тесты (5 штук, stub HTTP клиент)

**Критерий готовности:** `cargo test registry` — все 5 тестов.
**Коммит:** `feat: implement chain registry manager with SQLite cache`

---

### Шаг 5: Transport Layer (Модуль 2.3)

**Что делать:**
1. Создать `src-tauri/src/transport/cosmos_client.rs`
2. REST клиент: reqwest GET к 4 эндпоинтам Cosmos SDK
3. gRPC клиент: tonic с protobuf (cosmos.bank.v1beta1, cosmos.staking.v1beta1, cosmos.distribution.v1beta1)
4. Fallback chain: try gRPC → если ошибка/timeout → REST → cosmos.directory
5. Endpoint rotation: round-robin по доступным эндпоинтам
6. Timeout: 5 сек по умолчанию
7. Proxy support: передавать reqwest::Proxy в клиент
8. Парсинг ответов: JSON → структуры (Balance, Delegation, Reward, Unbonding)
9. Конвертация: uatom → ATOM (exponent из chain_tokens)
10. Написать тесты (10 штук, mock HTTP сервер)

**Критерий готовности:** `cargo test transport` — все 10 тестов.
**Коммит:** `feat: implement Cosmos API transport with gRPC/REST fallback`

---

### Шаг 6: Proxy Manager + File I/O (Модули 3.1, 3.2, 3.3)

**Что делать:**
1. `src-tauri/src/proxy/proxy_manager.rs` — парсинг txt, round-robin, health-check
2. `src-tauri/src/io/file_importer.rs` — парсинг txt → classify (address/seed/key)
3. `src-tauri/src/io/result_exporter.rs` — WalletResult → txt (format: `addr|chain|balance|staked|rewards|unbonding`)
4. Написать тесты (8 + 9 + 4 = 21 штука)

**Критерий готовности:** `cargo test proxy io` — все 21 тест.
**Коммит:** `feat: implement proxy manager, file importer, result exporter`

---

### Шаг 7: Checker Pipeline (Модуль 4.1)

**Что делать:**
1. `src-tauri/src/checker/pipeline.rs`
2. Оркестрация:
   - Для каждого WalletInput: derive addresses для всех выбранных сетей (Key Deriver)
   - Для каждого (address, chain): 4 параллельных запроса (Transport Layer)
   - Агрегация → WalletResult
   - Сохранение в SQLite по мере получения
3. Concurrency: `tokio::Semaphore(max_concurrency)`, tokio::spawn на каждый кошелёк
4. Progress events: Tauri app.emit("check:progress", ...)
5. Cancellation: tokio_util::sync::CancellationToken
6. Error handling: ошибки записываются в result.error, pipeline продолжает
7. Написать тесты (8 штук, mock Transport + mock Deriver)

**Критерий готовности:** `cargo test checker` — все 8 тестов, включая speed test.
**Коммит:** `feat: implement checker pipeline with async concurrency`

---

### Шаг 8: Tauri Commands (Command Layer)

**Что делать:**
1. `src-tauri/src/commands/*.rs` — обёртки для Tauri invoke
2. Каждый command из openapi.yaml: get_chains, import_wallets, import_proxies, start_check, stop_check, get_results, export_results, get_settings, update_settings
3. State management: Tauri `manage()` для shared state (DB, ProxyManager, etc.)
4. Error handling: anyhow → Tauri error → frontend получает readable message

**Критерий готовности:** Все commands доступны через invoke из frontend.
**Коммит:** `feat: implement Tauri IPC command handlers`

---

### Шаг 9: GUI (Модули 5.1-5.4)

**Что делать:**
1. **NetworkSelector** — список сетей с чекбоксами, поиск, select all
2. **WalletImport** — кнопка загрузки txt, summary (N addresses, M seeds, K keys)
3. **ProxyImport** — кнопка загрузки txt, summary (N valid, M invalid)
4. **Dashboard** — Start/Stop кнопки, прогресс-бар, скорость, таблица результатов (TanStack Table с виртуализацией), фильтры (all/with_funds/empty/errors), кнопка Export
5. **Settings** — concurrency slider, timeout, fallback toggle, cache refresh
6. Zustand store для состояния (chains, wallets, session, results)
7. Tailwind CSS для стилей
8. Написать component тесты (7 штук)

**Критерий готовности:** Полный UI flow работает в `cargo tauri dev`.
**Коммит:** `feat: implement GUI (network selector, import, dashboard, settings)`

---

### Шаг 10: Integration + Build (Модули 6.1, 6.2)

**Что делать:**
1. E2E тесты: полный flow (import → check → export) с реальными тестовыми адресами (пустые кошельки)
2. Performance test: 500 кошельков × 1 сеть, замерить скорость
3. `cargo tauri build` → .exe / .msi для Windows
4. Тестирование на чистой Windows 10/11

**Критерий готовности:** .exe запускается, полный flow работает, скорость ≥5000/мин.
**Коммит:** `release: v1.0.0 — Cosmos Checker initial release`

---

## Правила изоляции

- Каждый модуль в отдельной директории (`crypto/`, `registry/`, `transport/`, etc.)
- Модули общаются через типы из `models/`
- При разработке модуля N — соседние модули заменяются mock/stub
- Тесты модуля НЕ зависят от реальной сети/API

## Стратегия коммитов

- Один коммит на модуль (или логическую единицу)
- Формат: `feat: <что сделано>` / `fix: <что починено>` / `test: <что протестировано>`
- Коммит только после прохождения тестов модуля
- НЕ коммитить если тесты падают

## Правила прогона тестов

- После каждого модуля: `cargo test <module_name>`
- Перед integration: `cargo test` (все тесты)
- Перед build: `cargo test && npm test`

## Критерии перехода к следующему модулю

- Все тесты текущего модуля проходят
- Код компилируется без warnings
- Нет TODO/FIXME в коде текущего модуля

## Критерии остановки (ждать разработчика)

- Тест-вектор BIP39 не совпадает с ожидаемым результатом
- gRPC proto-файлы Cosmos SDK не компилируются
- Tauri IPC не работает (ошибка сериализации)
- Скорость проверки <2000 кошельков/мин на mock-тесте
- Любая ошибка компиляции, которую не удаётся исправить за 3 попытки
