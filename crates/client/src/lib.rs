#![allow(dead_code, private_interfaces)]

pub mod app;
pub mod i18n;
pub mod identity;
pub mod network;
pub mod routes;
pub mod services;
pub mod session;
pub mod storage;
pub mod theme;
pub mod ui;
#[cfg(target_os = "windows")]
pub mod windows_diagnostics;
