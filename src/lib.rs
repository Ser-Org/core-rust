//! Scout-core library facade. Exposes internal modules so integration tests
//! under `tests/` can exercise them directly.

pub mod app_state;
pub mod billing;
pub mod config;
pub mod db;
pub mod error;
pub mod financial;
pub mod flash;
pub mod handlers;
pub mod jobs;
pub mod logging;
pub mod media;
pub mod middleware;
pub mod models;
pub mod objectstore;
pub mod prompts;
pub mod providers;
pub mod repos;
pub mod router;
pub mod utils;
