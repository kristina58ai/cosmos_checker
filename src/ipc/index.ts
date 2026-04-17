// Thin wrapper over `@tauri-apps/api/core#invoke`.
// Все IPC-команды проходят через этот модуль — так легче менять типы
// и логировать вызовы централизованно.

import { invoke } from "@tauri-apps/api/core";

export async function ping(): Promise<string> {
  return invoke<string>("ping");
}

// Заготовки под команды из CLAUDE.md §4 — будут реализованы в Этапе 9.
// export async function getChains(forceRefresh: boolean): Promise<ChainInfo[]> { ... }
// export async function importWallets(filePath: string): Promise<ImportSummary> { ... }
// export async function startCheck(args: StartCheckArgs): Promise<{ session_id: string }> { ... }
// ...
