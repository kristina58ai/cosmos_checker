//! Key Deriver. Stage 2 (placeholder).

/// Конфиг сети для деривации (bech32-префикс + slip44).
#[derive(Debug, Clone)]
pub struct ChainConfig {
    pub bech32_prefix: String,
    pub slip44: u32,
}
