// Типы, зеркалирующие Rust-структуры из src-tauri/src/**.
// Наполняется по мере реализации этапов.

export interface WalletResult {
  address: string;
  chainId: string;
  hasFunds: boolean;
  balanceDisplay?: string;
  stakedDisplay?: string;
  rewardsDisplay?: string;
  unbondingDisplay?: string;
}

export type WalletInputKind = "address" | "seed" | "private_key";
