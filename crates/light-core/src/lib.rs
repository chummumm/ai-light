//! Transport-independent protocol, event reducer and snapshot aggregation.
//! No Bluetooth, HTTP or model API calls occur in this crate.
pub mod fsutil;
pub mod hooks;
pub mod model;
pub mod protocol;
pub mod battery;
pub mod network;
pub mod power;
pub mod telemetry;
pub mod sound;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis().min(u64::MAX as u128) as u64
}
