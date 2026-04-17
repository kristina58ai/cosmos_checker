# Cosmos Checker — Architecture

## Целевая платформа
- **OS:** Windows 10/11 (обязательно), macOS/Linux — бонус
- **Тип:** Десктоп-приложение с GUI
- **Фреймворк:** Tauri v2 (Rust backend + React frontend)

## Выбор тех-стека

### Почему Tauri v2 + Rust + React

| Критерий | Tauri v2 + Rust | Electron + Node.js | Flutter Desktop |
|----------|----------------|---------------------|-----------------|
| Скорость обработки | Rust — нативная скорость, zero-cost async | JS — медленнее в CPU-bound задачах | Dart — средняя скорость |
| RAM потребление | ~30-50 MB (WebView2) | ~200-400 MB (Chromium) | ~100-150 MB |
| Параллелизм | tokio — промышленный async runtime | libuv — ограниченный | Isolates — ограниченные |
| Крипто-библиотеки | k256, bip32, bip39 — нативные, быстрые | cosmjs — JS, медленнее | Нет зрелых Dart-библиотек |
| gRPC поддержка | tonic — нативный HTTP/2 | grpc-js — обёртка | grpc-dart — ограниченный |
| Windows | WebView2 (встроен в Win 10/11) | Chromium (bundled) | Win32 API |
| Размер билда | ~5-10 MB | ~150+ MB | ~20-30 MB |

**Вердикт:** Tauri v2 + Rust — оптимален по скорости, потреблению RAM и размеру. Rust обеспечивает 5000+ кошельков/мин за счёт нативного async и zero-cost abstractions.

### Rust-стек (Backend / Core)

| Назначение | Крейт | Версия | Обоснование |
|------------|-------|--------|-------------|
| Async runtime | `tokio` | 1.x | Промышленный стандарт, multi-threaded scheduler |
| HTTP клиент + прокси | `reqwest` | 0.12+ | Встроенная поддержка SOCKS5/HTTP proxy, connection pooling |
| gRPC клиент | `tonic` | 0.12+ | Нативный async gRPC over HTTP/2 |
| Protobuf | `prost` | 0.13+ | Кодогенерация из .proto файлов Cosmos SDK |
| BIP39 мнемоника | `bip39` | 2.x | Генерация seed из мнемоники |
| BIP32 HD-деривация | `bip32` | 0.5+ | HD-пути, поддержка secp256k1 через k256 |
| secp256k1 | `k256` | 0.13+ | Pure Rust, нет C-зависимостей |
| Bech32 | `bech32` | 0.11+ | Encode/decode Cosmos-адресов |
| SOCKS5 (расширенный) | `tokio-socks` | 0.5+ | Тонкий контроль над прокси |
| Сериализация | `serde` + `serde_json` | 1.x | JSON парсинг Chain Registry и API ответов |
| Конкурентность | `tokio::sync::Semaphore` | — | Ограничение параллельных соединений |
| Логирование | `tracing` | 0.1+ | Structured logging |
| Ошибки | `anyhow` + `thiserror` | — | Обработка ошибок |

### Frontend-стек (UI)

| Назначение | Технология | Обоснование |
|------------|-----------|-------------|
| UI фреймворк | React 18+ | Большая экосистема, компонентный подход |
| Стилизация | Tailwind CSS | Быстрая разработка UI |
| Состояние | Zustand | Легковесный state management |
| Таблицы | TanStack Table | Виртуализация для больших списков кошельков |
| Tauri API | @tauri-apps/api | Коммуникация frontend ↔ Rust backend |

## Архитектура приложения

### Слои

```
┌──────────────────────────────────────────────┐
│              GUI (React + Tailwind)           │
│  ┌─────────┐ ┌──────────┐ ┌───────────────┐  │
│  │ Network │ │ Wallet   │ │  Results      │  │
│  │ Selector│ │ Import   │ │  Dashboard    │  │
│  └────┬────┘ └────┬─────┘ └───────┬───────┘  │
│───────┼───────────┼───────────────┼──────────│
│       │     Tauri IPC (invoke)    │           │
│───────┼───────────┼───────────────┼──────────│
│              RUST BACKEND                     │
│  ┌────────────────────────────────────────┐   │
│  │         Command Layer (Tauri)          │   │
│  │  start_check, import_wallets,          │   │
│  │  get_chains, export_results            │   │
│  └────────────────┬───────────────────────┘   │
│                   │                           │
│  ┌────────────────┴───────────────────────┐   │
│  │         Core Engine                     │   │
│  │  ┌──────────┐  ┌───────────────────┐   │   │
│  │  │ Chain    │  │ Wallet Checker    │   │   │
│  │  │ Registry │  │ (async pipeline)  │   │   │
│  │  │ Manager  │  │                   │   │   │
│  │  └──────────┘  └───────────────────┘   │   │
│  │  ┌──────────┐  ┌───────────────────┐   │   │
│  │  │ Key      │  │ Proxy             │   │   │
│  │  │ Deriver  │  │ Manager           │   │   │
│  │  └──────────┘  └───────────────────┘   │   │
│  └────────────────┬───────────────────────┘   │
│                   │                           │
│  ┌────────────────┴───────────────────────┐   │
│  │       Transport Layer                   │   │
│  │  ┌────────┐ ┌────────┐ ┌────────────┐  │   │
│  │  │ gRPC   │ │ REST   │ │ cosmos.dir │  │   │
│  │  │ Client │ │ Client │ │ Client     │  │   │
│  │  └────────┘ └────────┘ └────────────┘  │   │
│  │       Fallback: gRPC → REST → cosmos.d  │   │
│  └─────────────────────────────────────────┘   │
└──────────────────────────────────────────────┘
```

### Ключевые модули

#### 1. Chain Registry Manager
- **Вход:** GitHub API / локальный кеш chain-registry
- **Выход:** `Vec<ChainConfig>` — список сетей с эндпоинтами, bech32-префиксами, slip44
- **Алгоритм:** HTTP GET → JSON parse → фильтрация mainnet → кеширование
- **O(n):** O(N) где N = количество сетей (~270), RAM ~5 MB

#### 2. Key Deriver
- **Вход:** seed-фраза или приватный ключ + `ChainConfig`
- **Выход:** bech32-адрес для конкретной сети
- **Алгоритм:** BIP39 → BIP32 derivation (m/44'/{slip44}'/0'/0/0) → secp256k1 pubkey → SHA256 → RIPEMD160 → Bech32
- **O(n):** O(1) на один адрес, ~0.1ms на деривацию, RAM ~1 KB
- **Для N кошельков × M сетей:** O(N×M), при 1000 кошельков × 10 сетей = 10000 деривации за ~1 сек

#### 3. Proxy Manager
- **Вход:** txt-файл с прокси (формат: `ip:port` или `ip:port:user:pass` или `socks5://ip:port`)
- **Выход:** round-robin итератор по прокси
- **Алгоритм:** парсинг → валидация → round-robin ротация с health-check
- **O(n):** O(1) на выдачу следующего прокси

#### 4. Wallet Checker (async pipeline)
- **Вход:** адрес + сеть + прокси
- **Выход:** `WalletResult` (балансы, стейкинг, rewards, unbonding)
- **Алгоритм:** 4 параллельных запроса на кошелёк (balances, delegations, rewards, unbonding) → агрегация
- **O(n):** O(1) на кошелёк, ~100-200ms latency per wallet
- **RAM:** ~2 KB на результат одного кошелька

#### 5. Transport Layer (Fallback Chain)
- **Приоритет:** gRPC → REST → cosmos.directory
- **Алгоритм:** попытка gRPC, при ошибке/таймауте (2 сек) → REST, при ошибке → cosmos.directory
- **Endpoint rotation:** для каждой сети — round-robin по доступным эндпоинтам из Chain Registry

### Модель конкурентности (5000 кошельков/мин)

```
Цель:         5000 wallets / 60 sec = ~84 wallets/sec
Запросов:     4 queries × 84 = 336 requests/sec
Latency:      ~100ms per request (среднее)
Concurrency:  336 × 0.1 = ~34 параллельных соединений (минимум)
Рабочее:      50-100 параллельных соединений (с запасом)
```

**Реализация:**
- `tokio::sync::Semaphore(100)` — ограничение до 100 параллельных запросов
- `tokio::spawn` на каждый кошелёк — 4 запроса внутри join_all
- Shared `reqwest::Client` с connection pooling
- Распределение по нескольким RPC-эндпоинтам для каждой сети
- Прокси-ротация для обхода rate limits

### Формат входных файлов

**wallets.txt** — один элемент на строку:
```
cosmos1abc...xyz                          # адрес
word1 word2 word3 ... word12              # seed 12 слов
word1 word2 word3 ... word24              # seed 24 слова
a1b2c3d4e5f6...                           # приватный ключ (hex, 64 символа)
```

**proxies.txt** — один прокси на строку:
```
ip:port
ip:port:username:password
socks5://ip:port
http://ip:port
http://username:password@ip:port
```

### Формат выходного файла

**results.txt** — по одной строке на кошелёк с данными:
```
address | network | balance | staked | rewards | unbonding
cosmos1abc...|cosmoshub|1.5 ATOM|10.0 ATOM|0.3 ATOM|0.0 ATOM
osmo1abc...|osmosis|25.0 OSMO|100.0 OSMO|5.2 OSMO|0.0 OSMO
```

### Локальная БД

SQLite (через `rusqlite`) для:
- Кеширование Chain Registry (чтобы не загружать каждый раз)
- Сохранение результатов проверок
- Хранение настроек приложения
- НЕ хранить seed-фразы и приватные ключи
