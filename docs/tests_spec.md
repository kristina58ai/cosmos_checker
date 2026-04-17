# Cosmos Checker — Test Specifications

## Общие правила
- Каждый модуль имеет изолированный тест-сьют
- Unit-тесты запускаются через `cargo test` (Rust) или `npm test` (React)
- Integration-тесты используют mock-серверы
- E2E-тесты проверяют полный flow через Tauri
- Тесты являются исполняемыми спецификациями

---

## Модуль 1.2: Key Deriver

### Unit Tests (`src/crypto/key_deriver_test.rs`)

```
TEST derive_address_from_seed_12_words
  GIVEN: seed = "abandon abandon abandon ... about" (тестовый вектор BIP39)
         chain = { bech32_prefix: "cosmos", slip44: 118 }
  WHEN:  derive_address(seed, chain)
  THEN:  result == "cosmos1..." (известный адрес из тестового вектора)

TEST derive_address_from_seed_24_words
  GIVEN: seed = 24 слова (тестовый вектор)
         chain = { bech32_prefix: "cosmos", slip44: 118 }
  WHEN:  derive_address(seed, chain)
  THEN:  result == ожидаемый cosmos1... адрес

TEST derive_address_from_private_key
  GIVEN: private_key = "a1b2c3..." (известный тестовый ключ)
         chain = { bech32_prefix: "cosmos", slip44: 118 }
  WHEN:  derive_address(private_key, chain)
  THEN:  result == ожидаемый адрес (сверка с CosmJS)

TEST derive_different_chains_from_same_seed
  GIVEN: один и тот же seed
         chains = [cosmos (118), osmosis (118), terra (330)]
  WHEN:  derive для каждой сети
  THEN:  cosmos1... != osmo1... (разные префиксы)
         cosmos1... и osmo1... имеют одинаковые 20 байт (если slip44 совпадает)
         terra1... отличается (slip44 = 330)

TEST derive_with_custom_slip44
  GIVEN: seed + chain с slip44 = 330 (Terra), bech32 = "terra"
  WHEN:  derive_address
  THEN:  адрес начинается с "terra1"
         адрес отличается от slip44=118

TEST invalid_seed_phrase_rejected
  GIVEN: seed = "invalid words here not a real mnemonic"
  WHEN:  derive_address
  THEN:  Err(InvalidMnemonic)

TEST zeroize_seed_after_derivation
  GIVEN: seed в SecretString
  WHEN:  SecretString dropped
  THEN:  память обнулена (проверка через unsafe raw pointer до и после drop)

TEST passthrough_address_input
  GIVEN: input = "cosmos1qwerty..."
  WHEN:  classify + derive
  THEN:  возвращает тот же адрес без изменений
```

---

## Модуль 2.1: Chain Registry Manager

### Unit Tests (`src/registry/chain_registry_test.rs`)

```
TEST parse_chain_json
  GIVEN: JSON файл cosmoshub/chain.json (фиксированный)
  WHEN:  parse_chain(json)
  THEN:  chain_id == "cosmoshub-4"
         bech32_prefix == "cosmos"
         slip44 == 118
         endpoints.grpc.len() >= 1

TEST parse_all_chains
  GIVEN: Набор из 10 тестовых chain.json (фиксированные)
  WHEN:  parse_registry(jsons)
  THEN:  result.len() == 10
         каждый имеет chain_id, bech32_prefix, slip44

TEST cache_chains_in_sqlite
  GIVEN: parsed chains + in-memory SQLite
  WHEN:  save_to_cache(chains) → load_from_cache()
  THEN:  loaded == original

TEST force_refresh_overwrites_cache
  GIVEN: кешированные данные v1
  WHEN:  fetch с force_refresh=true, данные v2
  THEN:  кеш содержит v2

TEST handle_missing_fields_gracefully
  GIVEN: chain.json без поля "apis.grpc"
  WHEN:  parse
  THEN:  endpoints.grpc == [] (пустой, не ошибка)
```

---

## Модуль 2.2: SQLite Database Layer

### Unit Tests (`src/db/database_test.rs`)

```
TEST create_tables_on_init
  GIVEN: пустая in-memory SQLite
  WHEN:  init_db()
  THEN:  все таблицы из schema.sql существуют

TEST insert_and_query_chain
  GIVEN: ChainConfig { chain_id: "test-1", ... }
  WHEN:  insert_chain → get_chain("test-1")
  THEN:  returned == inserted

TEST insert_and_query_results
  GIVEN: session + wallet_result
  WHEN:  create_session → insert_result → get_results(session_id)
  THEN:  results содержит вставленную запись

TEST prepared_statements_prevent_injection
  GIVEN: chain_id = "test'; DROP TABLE chains;--"
  WHEN:  insert_chain
  THEN:  НЕ удаляет таблицу, запись сохранена с "грязным" chain_id

TEST settings_crud
  GIVEN: key="max_concurrency", value="200"
  WHEN:  set → get
  THEN:  value == "200"
```

---

## Модуль 2.3: Transport Layer

### Unit Tests + Integration Tests (`src/transport/cosmos_client_test.rs`)

```
TEST parse_balance_response
  GIVEN: JSON ответ от /cosmos/bank/v1beta1/balances
         { "balances": [{"denom":"uatom","amount":"1500000"}] }
  WHEN:  parse_balances(json)
  THEN:  result == [Balance { denom: "uatom", amount: 1500000, display: "1.5 ATOM" }]

TEST parse_delegations_response
  GIVEN: JSON ответ от /cosmos/staking/v1beta1/delegations
  WHEN:  parse_delegations(json)
  THEN:  correctly parsed delegations with validator address and amount

TEST parse_rewards_response
  GIVEN: JSON ответ от /cosmos/distribution/v1beta1/delegators/.../rewards
  WHEN:  parse_rewards(json)
  THEN:  correctly parsed total rewards

TEST parse_unbonding_response
  GIVEN: JSON ответ от /cosmos/staking/v1beta1/delegators/.../unbonding_delegations
  WHEN:  parse_unbonding(json)
  THEN:  correctly parsed unbonding entries with completion_time

TEST fallback_grpc_to_rest
  GIVEN: mock gRPC server → returns error
         mock REST server → returns valid response
  WHEN:  check_wallet(addr, chain)
  THEN:  result == valid data from REST

TEST fallback_rest_to_cosmos_directory
  GIVEN: mock gRPC → error, mock REST → error
         mock cosmos.directory → valid response
  WHEN:  check_wallet
  THEN:  result from cosmos.directory

TEST all_endpoints_fail
  GIVEN: all mocks return errors
  WHEN:  check_wallet
  THEN:  Err(AllEndpointsFailed) с описанием

TEST timeout_handling
  GIVEN: mock server responds after 10sec, timeout=5sec
  WHEN:  check_wallet
  THEN:  ошибка таймаута, переход на следующий эндпоинт

TEST endpoint_rotation
  GIVEN: 3 endpoints, first always fails
  WHEN:  check_wallet 3 times
  THEN:  requests distributed across endpoint 2 and 3

TEST empty_balance_response
  GIVEN: адрес без балансов → { "balances": [] }
  WHEN:  parse_balances
  THEN:  result == [] (не ошибка)
```

---

## Модуль 3.1: Proxy Manager

### Unit Tests (`src/proxy/proxy_manager_test.rs`)

```
TEST parse_ip_port
  GIVEN: "1.2.3.4:8080"
  WHEN:  parse_proxy
  THEN:  ProxyConfig { host: "1.2.3.4", port: 8080, type: HTTP, auth: None }

TEST parse_ip_port_user_pass
  GIVEN: "1.2.3.4:8080:user:pass"
  WHEN:  parse_proxy
  THEN:  ProxyConfig { auth: Some("user", "pass") }

TEST parse_socks5_url
  GIVEN: "socks5://1.2.3.4:1080"
  WHEN:  parse_proxy
  THEN:  ProxyConfig { type: SOCKS5 }

TEST parse_http_url_with_auth
  GIVEN: "http://user:pass@1.2.3.4:8080"
  WHEN:  parse_proxy
  THEN:  ProxyConfig { type: HTTP, auth: Some("user", "pass") }

TEST round_robin_rotation
  GIVEN: [proxy1, proxy2, proxy3]
  WHEN:  next() × 6
  THEN:  p1, p2, p3, p1, p2, p3

TEST skip_unhealthy_proxy
  GIVEN: [proxy1(healthy), proxy2(unhealthy), proxy3(healthy)]
  WHEN:  next() × 4
  THEN:  p1, p3, p1, p3

TEST no_proxies_returns_none
  GIVEN: пустой список прокси
  WHEN:  next()
  THEN:  None (запрос идёт напрямую)

TEST invalid_proxy_format
  GIVEN: "not-a-proxy"
  WHEN:  parse_proxy
  THEN:  Err(InvalidProxyFormat)
```

---

## Модуль 3.2: File Importer

### Unit Tests (`src/io/file_importer_test.rs`)

```
TEST classify_bech32_address
  GIVEN: "cosmos1abc123xyz..."
  WHEN:  classify_line
  THEN:  WalletInput::Address

TEST classify_seed_12_words
  GIVEN: "word1 word2 word3 ... word12" (валидные BIP39 слова)
  WHEN:  classify_line
  THEN:  WalletInput::Seed

TEST classify_seed_24_words
  GIVEN: 24 BIP39 слова
  WHEN:  classify_line
  THEN:  WalletInput::Seed

TEST classify_private_key_hex
  GIVEN: 64 hex символа
  WHEN:  classify_line
  THEN:  WalletInput::PrivateKey

TEST skip_empty_lines
  GIVEN: файл с пустыми строками между данными
  WHEN:  import_file
  THEN:  пустые строки игнорируются

TEST skip_comments
  GIVEN: "# this is a comment"
  WHEN:  classify_line
  THEN:  пропущено

TEST report_invalid_lines
  GIVEN: файл с 10 строк, 2 невалидных
  WHEN:  import_file
  THEN:  result.wallets.len() == 8
         result.invalid.len() == 2
         result.invalid содержит номера строк

TEST invalid_bip39_words
  GIVEN: "apple banana cherry ..." (12 слов, но не из BIP39 словаря)
  WHEN:  classify_line
  THEN:  Invalid (не Seed)

TEST mixed_file
  GIVEN: файл с адресами, seed-фразами, ключами, пустыми строками, комментариями
  WHEN:  import_file
  THEN:  правильная классификация каждого типа
```

---

## Модуль 3.3: Result Exporter

### Unit Tests (`src/io/result_exporter_test.rs`)

```
TEST export_all_results
  GIVEN: 3 WalletResult (2 with funds, 1 empty)
  WHEN:  export(results, filter=all)
  THEN:  файл содержит 3 строки данных + заголовок

TEST export_only_with_funds
  GIVEN: 3 WalletResult (2 with funds, 1 empty)
  WHEN:  export(results, filter=with_funds)
  THEN:  файл содержит 2 строки данных + заголовок

TEST export_format
  GIVEN: WalletResult { address: "cosmos1...", chain: "cosmoshub", balance: "1.5 ATOM", ... }
  WHEN:  export
  THEN:  строка == "cosmos1...|cosmoshub|1.5 ATOM|10.0 ATOM|0.3 ATOM|0.0 ATOM"

TEST export_header
  WHEN:  export(any)
  THEN:  первая строка == "address|chain|balance|staked|rewards|unbonding"
```

---

## Модуль 4.1: Checker Pipeline

### Integration Tests (`src/checker/pipeline_test.rs`)

```
TEST check_single_wallet_single_chain
  GIVEN: 1 адрес + 1 сеть (mock transport)
  WHEN:  run_check
  THEN:  1 WalletResult с правильными данными

TEST check_seed_multiple_chains
  GIVEN: 1 seed + 3 сети (mock transport)
  WHEN:  run_check
  THEN:  3 WalletResult (один адрес на каждую сеть)

TEST concurrency_respects_semaphore
  GIVEN: 100 кошельков, semaphore = 10
  WHEN:  run_check
  THEN:  максимум 10 одновременных запросов (mock server tracks concurrency)

TEST speed_target_5000_per_min
  GIVEN: 500 кошельков + 1 сеть, mock transport (1ms latency)
  WHEN:  run_check, measure time
  THEN:  elapsed < 6 seconds (проекция 5000/мин)

TEST progress_events_emitted
  GIVEN: 10 кошельков, event listener
  WHEN:  run_check
  THEN:  получено ≥10 progress events с checked/total

TEST graceful_cancellation
  GIVEN: 1000 кошельков, cancel after 100
  WHEN:  run_check + cancel
  THEN:  pipeline останавливается, checked ≈ 100, status = cancelled

TEST proxy_rotation_during_check
  GIVEN: 5 кошельков + 2 прокси (mock)
  WHEN:  run_check
  THEN:  запросы распределены между прокси

TEST errors_dont_stop_pipeline
  GIVEN: 10 кошельков, 3 из них возвращают ошибки
  WHEN:  run_check
  THEN:  7 успешных + 3 с ошибками, pipeline не падает
```

---

## GUI Modules (5.1 - 5.4)

### Component Tests (Jest + React Testing Library)

```
TEST NetworkSelector_renders_chains
  GIVEN: mock getChains → [cosmos, osmosis, juno]
  WHEN:  render <NetworkSelector>
  THEN:  3 chain cards visible

TEST NetworkSelector_select_all
  GIVEN: rendered, none selected
  WHEN:  click "Select All"
  THEN:  all chains checked

TEST NetworkSelector_search_filter
  GIVEN: rendered with 10 chains
  WHEN:  type "osmo" in search
  THEN:  only osmosis visible

TEST WalletImport_shows_summary
  GIVEN: mock importWallets → { addresses: 5, seeds: 3, private_keys: 2 }
  WHEN:  render + select file
  THEN:  summary shows "5 addresses, 3 seeds, 2 private keys"

TEST Dashboard_progress_bar
  GIVEN: mock events: checked=50, total=100
  WHEN:  render <Dashboard>
  THEN:  progress bar at 50%, speed displayed

TEST Dashboard_results_table
  GIVEN: mock getResults → 5 results
  WHEN:  render
  THEN:  table with 5 rows, correct columns

TEST Dashboard_filter_with_funds
  GIVEN: 5 results (3 with funds)
  WHEN:  click "With Funds" filter
  THEN:  3 rows visible
```

---

## E2E Tests

```
TEST full_flow_address_check
  GIVEN: Tauri app running
  WHEN:  select chains → import wallets.txt (addresses) → start check → export
  THEN:  results.txt contains correct data

TEST full_flow_seed_check
  GIVEN: Tauri app running
  WHEN:  select 2 chains → import seeds.txt → start check
  THEN:  results contain addresses for both chains per seed

TEST full_flow_with_proxy
  GIVEN: Tauri app running + proxy server
  WHEN:  import proxies.txt → start check
  THEN:  requests go through proxy (verify on proxy server logs)
```
