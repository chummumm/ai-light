//! Voltage measurement only. READ 0x14 is a little-endian u16 in millivolts,
//! as used by the compatible device's desktop client. No charging inference.
use serde::{Deserialize, Serialize};
pub const SAMPLE_INTERVAL_MS: u64 = 15_000;
pub const MAX_GAP_MS: u64 = 45_000;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PowerSample {
    pub voltage_mv: u16,
    pub raw_14: String,
}
