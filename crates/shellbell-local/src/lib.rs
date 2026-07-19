//! Per-user Linux/WSL activity monitoring, local IPC, and managed shell integration.

pub mod activity;
pub mod config;
pub mod daemon;
pub mod install;
pub mod ipc;
pub mod store;

pub use activity::{ActivityAction, ActivityEngine, Mode, Session, SessionState, ShellType};
pub use config::{
    EffectiveConfig, LocalPaths, StoredCredentials, load_credentials, save_credentials,
};
