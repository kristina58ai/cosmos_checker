# Cosmos Checker — Development Plan

## Этапы разработки

Каждый модуль проектируется изолированно: чёткие входы, выходы, интерфейсы. Модуль разрабатывается и тестируется независимо от остальных через mock/stub.

---

## Этап 1: Scaffolding + Key Deriver

### Модуль 1.1: Tauri Project Scaffold
- **Описание:** Инициализация Tauri v2 проекта с Rust backend и React frontend
- **Входы:** Нет
- **Выходы:** Рабочий Tauri-проект, собирается и запускается на Windows
- **Зависимости:** Нет
- **Acceptance criteria:**
  - `cargo tauri dev` запускает окно приложения
  - React frontend рендерит placeholder UI
  - Tauri IPC работает: frontend вызывает `invoke("greet")` → backend отвечает
- **Mock/Stub:** Нет

### Модуль 1.2: Key Deriver
- **Описание:** Деривация Cosmos-адресов из seed-фраз и приватных ключей
- **Входы:** `enum WalletInput { Address(String), Seed(SecretString), PrivateKey(SecretBytes) }` + `ChainConfig { bech32_prefix, slip44 }`
- **Выходы:** `String` — bech32-адрес
- **Зависимости:** Нет (чистая криптография)
- **Acceptance criteria:**
  - Из известного seed → правильный cosmos1... адрес (сверка с Keplr/CosmJS)
  - Из приватного ключа → тот же адрес что даёт CosmJS
  - Разные slip44 → разные адреса
  - Разные bech32_prefix → правильные префиксы (cosmos, osmo, juno...)
  - `zeroize` обнуляет seed/ключ при Drop
- **Mock/Stub:** `ChainConfig` — статическая структура, не требует сети

---

## Этап 2: Chain Registry + Transport Layer

### Модуль 2.1: Chain Registry Manager
- **Описание:** Загрузка и кеширование списка Cosmos-сетей из GitHub chain-registry
- **Входы:** URL GitHub API (или локальный файл для тестов)
- **Выходы:** `Vec<ChainConfig>` — список сетей с эндпоинтами
- **Зависимости:** SQLite (для кеша)
- **Acceptance criteria:**
  - Загружает ≥200 сетей из chain-registry
  - Парсит chain_id, bech32_prefix, slip44, endpoints (grpc/rest/rpc)
  - Кеширует в SQLite, при повторном запуске читает из кеша
  - `force_refresh` обновляет кеш с GitHub
  - Корректно обрабатывает сети с нестандартным slip44
- **Mock/Stub для соседей:** Stub HTTP-клиент, возвращающий фиксированный JSON

### Модуль 2.2: SQLite Database Layer
- **Описание:** Инициализация и миграции SQLite, CRUD-операции
- **Входы:** Путь к файлу БД
- **Выходы:** Database handle с методами insert/query/update
- **Зависимости:** Нет
- **Acceptance criteria:**
  - Создаёт все таблицы из schema.sql при первом запуске
  - Все запросы через prepared statements
  - CRUD для chains, endpoints, sessions, results, settings
- **Mock/Stub:** In-memory SQLite для тестов

### Модуль 2.3: Transport Layer (Cosmos API Client)
- **Описание:** Клиент для запросов к Cosmos SDK нодам с fallback chain
- **Входы:** `address` + `ChainConfig` + `Option<Proxy>`
- **Выходы:** `WalletData { balances, delegations, rewards, unbonding }`
- **Зависимости:** Нет (принимает endpoint URLs как параметры)
- **Acceptance criteria:**
  - REST: запрос `/cosmos/bank/v1beta1/balances/{addr}` → парсинг ответа
  - REST: запрос `/cosmos/staking/v1beta1/delegations/{addr}` → парсинг
  - REST: запрос `/cosmos/distribution/v1beta1/delegators/{addr}/rewards` → парсинг
  - REST: запрос `/cosmos/staking/v1beta1/delegators/{addr}/unbonding_delegations` → парсинг
  - gRPC: аналогичные запросы через tonic
  - Fallback: gRPC fail → REST → cosmos.directory
  - Таймаут: 5 сек на запрос, retry с другим эндпоинтом
  - Endpoint rotation: round-robin по доступным эндпоинтам
- **Mock/Stub:** Mock HTTP/gRPC сервер с фиксированными ответами (формат Cosmos SDK)

---

## Этап 3: Proxy Manager + File I/O

### Модуль 3.1: Proxy Manager
- **Описание:** Импорт, валидация и ротация прокси
- **Входы:** txt-файл с прокси
- **Выходы:** `fn next_proxy() -> Option<ProxyConfig>`
- **Зависимости:** Нет
- **Acceptance criteria:**
  - Парсит форматы: `ip:port`, `ip:port:user:pass`, `socks5://ip:port`, `http://user:pass@ip:port`
  - Round-robin ротация
  - Health-check: пометить нерабочие, пропускать их
  - Работает без прокси (прокси опциональны)
- **Mock/Stub:** Нет (чистая логика)

### Модуль 3.2: File Importer
- **Описание:** Парсинг txt-файлов с кошельками
- **Входы:** Путь к txt-файлу
- **Выходы:** `Vec<WalletInput>` — распознанные адреса/seed/ключи + список ошибок
- **Зависимости:** Нет
- **Acceptance criteria:**
  - Определяет тип строки: bech32 адрес (regex), seed 12/24 слова, hex приватный ключ (64 символа)
  - Пропускает пустые строки и комментарии (#)
  - Возвращает номера невалидных строк
  - Seed-фразы валидируются по BIP39 wordlist
- **Mock/Stub:** Нет (чистая логика)

### Модуль 3.3: Result Exporter
- **Описание:** Экспорт результатов в txt
- **Входы:** `Vec<WalletResult>` + путь к файлу + фильтр (all/with_funds/empty)
- **Выходы:** txt-файл
- **Зависимости:** Нет
- **Acceptance criteria:**
  - Формат: `address|chain|balance|staked|rewards|unbonding`
  - Фильтр: only with_funds, only empty, all
  - Корректная обработка UTF-8
  - Заголовок в первой строке
- **Mock/Stub:** Нет (чистая логика)

---

## Этап 4: Wallet Checker Engine

### Модуль 4.1: Checker Pipeline (Orchestrator)
- **Описание:** Оркестрация массовой проверки: деривация → запросы → агрегация
- **Входы:** `Vec<WalletInput>` + `Vec<ChainConfig>` + `ProxyManager` + settings
- **Выходы:** `Vec<WalletResult>` + progress events
- **Зависимости:** Key Deriver (1.2), Transport Layer (2.3), Proxy Manager (3.1)
- **Acceptance criteria:**
  - Скорость: ≥5000 кошельков/мин при 100 concurrency
  - tokio::Semaphore ограничивает параллелизм
  - Progress events через Tauri emit: checked/total/speed
  - Graceful stop: отмена через CancellationToken
  - Результаты сохраняются в SQLite по мере получения
  - Seed → деривация адресов для всех выбранных сетей → проверка каждого
- **Mock/Stub:** Mock Transport Layer (фиксированные ответы), Mock Key Deriver

---

## Этап 5: GUI

### Модуль 5.1: Network Selector UI
- **Описание:** Экран выбора Cosmos-сетей
- **Входы:** Tauri invoke `get_chains` → список сетей
- **Выходы:** `selected_chain_ids: string[]`
- **Зависимости:** Chain Registry Manager (2.1) через Tauri IPC
- **Acceptance criteria:**
  - Список всех сетей с иконками и названиями
  - Поиск/фильтрация по названию
  - Select all / deselect all
  - Показывает количество доступных эндпоинтов для каждой сети
- **Mock/Stub:** Фиксированный JSON-ответ от invoke

### Модуль 5.2: Wallet & Proxy Import UI
- **Описание:** Экран импорта файлов
- **Входы:** Пользователь выбирает txt-файлы через file dialog
- **Выходы:** Tauri invoke `import_wallets` / `import_proxies`
- **Зависимости:** File Importer (3.2), Proxy Manager (3.1) через Tauri IPC
- **Acceptance criteria:**
  - File dialog для выбора txt
  - Показывает сводку: N адресов, M seed-фраз, K приватных ключей
  - Показывает ошибки парсинга (невалидные строки)
  - Прокси: N valid, M invalid, типы (HTTP/SOCKS5)
- **Mock/Stub:** Mock Tauri invoke

### Модуль 5.3: Check Dashboard UI
- **Описание:** Экран запуска проверки и отображения прогресса/результатов
- **Входы:** Tauri events (progress, results) + invoke `get_results`
- **Выходы:** Отображение таблицы результатов + кнопки Start/Stop/Export
- **Зависимости:** Checker Pipeline (4.1) через Tauri IPC/events
- **Acceptance criteria:**
  - Кнопка Start / Stop
  - Real-time прогресс: checked/total, скорость (кошельков/мин)
  - Таблица результатов с виртуализацией (TanStack Table)
  - Фильтры: all / with funds / empty / errors
  - Кнопка Export → file dialog → txt
  - Цветовая индикация: зелёный = есть средства, серый = пусто, красный = ошибка
- **Mock/Stub:** Mock Tauri events

### Модуль 5.4: Settings UI
- **Описание:** Экран настроек
- **Входы/Выходы:** Tauri invoke `get_settings` / `update_settings`
- **Acceptance criteria:**
  - Слайдер concurrency (10-500)
  - Таймаут запроса (1-30 сек)
  - Fallback вкл/выкл
  - Кеш Chain Registry (часы)
  - Кнопка обновить Chain Registry
- **Mock/Stub:** Mock Tauri invoke

---

## Этап 6: Integration + Polish

### Модуль 6.1: End-to-End Integration
- **Описание:** Связать все модули, e2e тестирование
- **Зависимости:** Все предыдущие модули
- **Acceptance criteria:**
  - Полный flow: запуск → выбор сетей → импорт → проверка → экспорт
  - Скорость ≥5000 кошельков/мин с реальными API
  - Стабильность: нет crash при 10000+ кошельках
  - Windows: сборка через `cargo tauri build` → .msi/.exe

### Модуль 6.2: Windows Build & Distribution
- **Описание:** Финальная сборка, подпись, тестирование на чистой Windows
- **Acceptance criteria:**
  - .exe / .msi установщик
  - Запуск на чистой Windows 10/11 без дополнительных зависимостей
  - WebView2 bootstrapper включён в установщик

---

## Граф зависимостей между модулями

```
1.1 Scaffold ─────────────────────────────────────┐
1.2 Key Deriver (нет зависимостей) ──────────┐    │
                                              │    │
2.1 Chain Registry ──┐                        │    │
2.2 SQLite Layer ────┤                        │    │
2.3 Transport Layer ─┤                        │    │
                     │                        │    │
3.1 Proxy Manager ───┤                        │    │
3.2 File Importer ───┤                        │    │
3.3 Result Exporter ─┘                        │    │
                                              │    │
4.1 Checker Pipeline ◄───────(2.3 + 1.2 + 3.1)    │
                                              │    │
5.1 Network Selector UI ◄──────────(2.1 + 1.1)    │
5.2 Import UI ◄────────────(3.2 + 3.1 + 1.1)     │
5.3 Dashboard UI ◄─────────────(4.1 + 1.1)        │
5.4 Settings UI ◄──────────────(2.2 + 1.1)        │
                                                   │
6.1 Integration ◄────────────────(all modules)     │
6.2 Windows Build ◄─────────────────(6.1)──────────┘
```
