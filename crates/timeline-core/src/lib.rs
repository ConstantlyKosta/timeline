//! Timeline core: platform-neutral logic shared by the desktop/mobile app and the dev bridge.
//!
//! Layers: `models` (data) → `db` (SQLite) → `library` (service) → `dispatch` (JSON commands).
//! `timeline` holds the zoom/cluster/layout math (mirrored in TypeScript for per-frame zooming).

pub mod db;
pub mod dispatch;
pub mod error;
pub mod graph;
pub mod library;
pub mod markdown;
pub mod media;
pub mod models;
pub mod sample;
pub mod search;
pub mod timeline;
pub mod transcript;

pub use error::{Error, Result};
pub use library::Library;
