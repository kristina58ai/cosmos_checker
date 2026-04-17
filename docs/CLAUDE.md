# CLAUDE.md — Cosmos Checker

> Этот файл — единственный документ, который нужно прочитать Claude Code перед началом разработки. Он самодостаточен.

---

## 1. Project Overview

**Cosmos Checker** — десктоп-приложение (Windows) с GUI для массовой проверки криптовалютных кошельков в экосистеме Cosmos SDK.

**Ключевая задача:** пользователь загружает txt-файл с кошельками (адреса / seed-фразы / приватные ключи), выбирает Cosmos-сети, загружает прокси, запускает проверку и получает отчёт по балансам и стейкингу. Скорость: ≥5000 кошельков/мин.

**Скоуп:**
- Read-only — никаких транзакций
- Только Cosmos SDK сети
- Данные: native-балансы, IBC-токены, delegations, rewards, unbonding
- Вход: txt, выход: txt

**Что НЕ входит:** отправка TX, DEX, история транзакций, governance, валидаторы, не-Cosmos сети.

---

## 2. Architecture & Tech Stack

### Стек
- **Framework:** Tauri v2 (Rust backend + React frontend)
- **Runtime:** tokio (async, multi-threaded)
- **HTTP:** reqwest 0.12+ (connection pooling, SOCKS5/HTTP proxy)
- **gRPC:** tonic 0.12+ (HTTP/2, protobuf через prost)
- **Crypto:** bip39 2.x, bip32 0.5+, k256 0.13+, bech32 0.11+
- **Security:** zeroize, secrecy (Secret<T>)
- **DB:** rusqlite (SQLite)
- **Frontend:** React 18+, Tailwind CSS, Zustand, TanStack Table
- **Логирование:** tracing

### Архитектура слоёв
```
GUI (React) ←→ Tauri IPC ←→ Command Layer ←→ Core Engine ←→ Transport Layer
                                                              ↓
                                                    gRPC → REST → cosmos.directory
```

### Модули
1. **Key Deriver** — BIP39/BIP32 → secp256k1 → Bech32 адреса
2. **Chain Registry Manager** — загрузка/кеш сетей из cosmos/chain-registry
3. **SQLite Layer** — кеш, результаты, настройки
4. **Transport Layer** — Cosmos API клиент с fallback (gRPC → REST → cosmos.directory)
5. **Proxy Manager** — импорт/ротация прокси (HTTP, SOCKS5)
6. **File Importer** — парсинг txt (адреса, seed, приватные ключи)
7. **Result Exporter** — экспорт в txt
8. **Checker Pipeline** — оркестрация массовой проверки (tokio::Semaphore, 100 concurrent)
9. **GUI** — Network Selector, Import, Dashboard, Settings

---

## 3. Database Schema

SQLite. Таблицы:
- `chains` — кеш сетей (chain_id PK, bech32_prefix, slip44)
- `chain_endpoints` — эндпоинты (grpc/rest/rpc) с health status
- `chain_tokens` — деноминации (uatom → ATOM, exponent=6)
- `check_sessions` — сессии проверки (status: running/completed/cancelled)
- `wallet_results` — результаты (address, chain_id, balance/staked/rewards/unbonding в raw JSON + display string, has_funds flag)
- `app_settings` — key-value настройки

**КРИТИЧНО:** seed-фразы и приватные ключи НИКОГДА не записываются в БД. Только bech32-адреса.

---

## 4. API Contract (Tauri IPC Commands)

| Command | Вход | Выход |
|---------|------|-------|
| `get_chains` | `{ force_refresh: bool }` | `Vec<ChainInfo>` |
| `get_chain_details` | `{ chain_id }` | `ChainInfo` |
| `import_wallets` | `{ file_path }` | `{ total, addresses, seeds, private_keys, invalid, invalid_lines }` |
| `import_proxies` | `{ file_path }` | `{ total, valid, invalid, types }` |
| `start_check` | `{ chain_ids, max_concurrency }` | `{ session_id }` |
| `stop_check` | `{ session_id }` | `{ status: "cancelled" }` |
| `get_results` | `{ session_id, filter, page, page_size }` | `ResultsPage` |
| `export_results` | `{ session_id, file_path, filter }` | `{ exported_count, file_path }` |
| `get_settings` / `update_settings` | `AppSettings` | `AppSettings` |

**Events (backend → frontend):**
- `check:progress` → `{ checked, total, speed, current_address }`
- `check:result` → `{ wallet_result }`
- `check:error` → `{ address, chain_id, error }`
- `check:completed` → `{ session_id, total, with_funds, duration_sec }`

---

## 5. Security Model

### Главные правила:
1. **Seed/ключи — только RAM.** Обёрнуты в `Secret<T>` (secrecy крейт), обнуляются при Drop (zeroize)
2. **TLS everywhere.** Не отключать верификацию сертификатов
3. **Prepared statements.** Все SQL через rusqlite параметризованные запросы
4. **Нет логирования секретов.** Secret<T> выводит `[REDACTED]` при Debug/Display
5. **Минимальные Tauri permissions.** Только необходимые API в tauri.conf.json

### Threat Model:
- T1 (Утечка ключей с диска): RAM only + zeroize
- T2 (Утечка через swap): zeroize + secrecy
- T3 (MITM на RPC): TLS обязателен
- T4 (Вредоносный прокси): через прокси идут только публичные адреса
- T5 (Утечка в логах): Secret<T>
- T6 (SQL injection): prepared statements
- T7 (DoS от нод): timeout 5s + fallback + rotation

---

## 6. Development Plan

### Порядок разработки:

**Этап 1:** Scaffold + Key Deriver
- 1.1 Tauri v2 project init (React + Rust)
- 1.2 Key Deriver (BIP39 → BIP32 → secp256k1 → Bech32)

**Этап 2:** Chain Registry + Transport
- 2.1 Chain Registry Manager (GitHub → parse → SQLite cache)
- 2.2 SQLite Database Layer (init, migrations, CRUD)
- 2.3 Transport Layer (gRPC/REST client + fallback chain)

**Этап 3:** Proxy + File I/O
- 3.1 Proxy Manager (parse, rotate, health-check)
- 3.2 File Importer (txt → classify → WalletInput)
- 3.3 Result Exporter (WalletResult → txt)

**Этап 4:** Checker Engine
- 4.1 Checker Pipeline (orchestrator: derive → query → aggregate, Semaphore(100))

**Этап 5:** GUI
- 5.1 Network Selector (chain list + search + select)
- 5.2 Wallet & Proxy Import (file dialog + summary)
- 5.3 Check Dashboard (progress + results table + export)
- 5.4 Settings

**Этап 6:** Integration + Build
- 6.1 E2E integration tests
- 6.2 Windows build (.exe/.msi)

---

## 7. Test Specifications

67 тестов по всем модулям:
- Key Deriver: 8 unit tests (derivation vectors, zeroize, invalid input)
- Chain Registry: 5 unit tests (parse, cache, refresh)
- SQLite: 5 unit tests (CRUD, injection prevention)
- Transport: 10 unit+integration tests (parse responses, fallback, timeout, rotation)
- Proxy Manager: 8 unit tests (parse formats, rotation, health)
- File Importer: 9 unit tests (classify types, validation, edge cases)
- Result Exporter: 4 unit tests (format, filter, header)
- Checker Pipeline: 8 integration tests (speed, concurrency, cancellation, errors)
- GUI: 7 component tests (render, interaction)
- E2E: 3 tests (full flow with addresses, seeds, proxies)

---

## 8. Decisions Log

| Решение | Обоснование |
|---------|-------------|
| **Tauri v2** вместо Electron | RAM: 30-50 MB vs 200-400 MB. Rust backend — нативная скорость для 5000 wallet/min |
| **Rust** вместо Node.js | Zero-cost async через tokio, нативные крипто-крейты (k256), нет GC pauses |
| **gRPC (tonic)** как основной протокол | HTTP/2 мультиплексирование, Protobuf — на 20-30% быстрее REST |
| **REST fallback** | Не все ноды имеют gRPC эндпоинты; REST через gRPC-gateway доступен шире |
| **cosmos.directory** как третий fallback | Встроенный load balancing и health-check, не требует ручного выбора нод |
| **SQLite** вместо файлового кеша | ACID, prepared statements (security), эффективные запросы по результатам |
| **tokio::Semaphore(100)** | 100 concurrent connections достаточно для 5000 wallets/min при ~100ms latency |
| **Secret<T> + zeroize** | Seed-фразы и ключи — критически sensitive; предотвращает утечку через логи, swap, crash dumps |
| **txt вход/выход** | Требование пользователя; простой формат, совместим с другими инструментами |
| **TanStack Table** для результатов | Виртуализация — рендер 100K+ строк без тормозов |
| **Round-robin** endpoint rotation | Распределяет нагрузку, обходит rate limits на отдельных нодах |
| **Chain Registry от GitHub** | Единый источник правды для 270+ Cosmos-сетей, обновляется сообществом |

---

## Cosmos SDK API Endpoints (Reference)

```
GET /cosmos/bank/v1beta1/balances/{address}
GET /cosmos/staking/v1beta1/delegations/{delegatorAddr}
GET /cosmos/distribution/v1beta1/delegators/{delegator_address}/rewards
GET /cosmos/staking/v1beta1/delegators/{delegator_address}/unbonding_delegations
```

gRPC equivalents:
```
cosmos.bank.v1beta1.Query/AllBalances
cosmos.staking.v1beta1.Query/DelegatorDelegations
cosmos.distribution.v1beta1.Query/DelegationTotalRewards
cosmos.staking.v1beta1.Query/DelegatorUnbondingDelegations
```

---

## Key Derivation Path

```
Seed (BIP39) → PBKDF2-HMAC-SHA512 → Master Key (BIP32)
  → m/44'/{slip44}'/0'/0/0
    → Private Key (secp256k1)
      → Public Key (compressed, 33 bytes)
        → SHA256 → RIPEMD160 → 20 bytes
          → Bech32({prefix}, 20_bytes) → "cosmos1..."
```

slip44 и bech32_prefix берутся из Chain Registry для каждой сети.
