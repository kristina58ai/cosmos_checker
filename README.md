# Cosmos Checker

Desktop-приложение (Windows) для массовой проверки криптовалютных кошельков в экосистеме Cosmos SDK.

- **Вход:** txt с адресами / seed-фразами / приватными ключами
- **Выход:** txt-отчёт с балансами, стейкингом, rewards, unbonding
- **Скорость:** ≥ 5 000 кошельков в минуту
- **Режим:** read-only, никаких транзакций

## Стек

- Tauri v2 (Rust backend + React frontend)
- tokio, reqwest, tonic (gRPC), rusqlite (SQLite)
- bip39 / bip32 / k256 / bech32 / secrecy + zeroize
- React 18, Tailwind, Zustand, TanStack Table

Полная спецификация — в [`docs/CLAUDE.md`](docs/CLAUDE.md).

## Быстрый старт (dev)

```bash
# prerequisites: Rust ≥ 1.80, Node ≥ 20, pnpm, cargo-tauri-cli
pnpm install
cargo tauri dev
```

## Сборка Windows-артефакта

```bash
cargo tauri build
# → src-tauri/target/release/bundle/msi/*.msi
```

## Тесты

```bash
cargo test            # Rust backend
pnpm test             # React frontend
```

## Статус

Генерация проекта идёт поэтапно. План — [`docs/plan.md`](docs/plan.md).

- [x] Этап 0 — Окружение и CI
- [x] Этап 1 — Скаффолд
- [ ] Этап 2 — Key Deriver
- [ ] Этап 3 — SQLite
- [ ] Этап 4 — Chain Registry
- [ ] Этап 5 — Transport (REST + gRPC)
- [ ] Этап 6 — Proxy Manager
- [ ] Этап 7 — File I/O
- [ ] Этап 8 — Checker Pipeline
- [ ] Этап 9 — Tauri IPC
- [ ] Этап 10 — React UI
- [ ] Этап 11 — E2E + Windows build

## Лицензия

MIT
