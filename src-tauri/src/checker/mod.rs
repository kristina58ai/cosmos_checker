//! Checker Pipeline (Stage 8).
//!
//! Пайплайн оркестрации массовой проверки кошельков:
//! входы → derive (crypto) → query (transport) → aggregate → persist (db)
//! → progress (mpsc).
//!
//! См. [`pipeline::run_pipeline`] для точки входа. События прогресса —
//! [`pipeline::ProgressEvent`] — эмитятся в канал, который в Stage 9
//! проксируется через `tauri::AppHandle::emit("check:progress", ...)`.

pub mod pipeline;

pub use pipeline::{run_pipeline, PipelineConfig, PipelineSummary, ProgressEvent};
