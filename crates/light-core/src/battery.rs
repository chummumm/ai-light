//! Standard BLE Battery Level only. No charging fields or inference.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BatteryReading {
    pub percent: Option<u8>,
    pub source: String,
    pub note: String,
    pub characteristics: Vec<String>,
}
pub fn parse_level(bytes: &[u8]) -> Result<u8> {
    ensure!(bytes.len() == 1 && bytes[0] <= 100,
        "invalid Battery Level (expected one byte, 0..100)");
    Ok(bytes[0])
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn zero_is_valid(){assert_eq!(parse_level(&[0]).unwrap(),0);}
    #[test] fn hundred_is_valid(){assert_eq!(parse_level(&[100]).unwrap(),100);}
    #[test] fn reserved_rejected(){assert!(parse_level(&[255]).is_err());}
    #[test] fn empty_rejected(){assert!(parse_level(&[]).is_err());}
    #[test] fn extra_bytes_rejected(){assert!(parse_level(&[30,1]).is_err());}
    #[test] fn no_charge_fields(){let json=serde_json::to_string(&BatteryReading::default()).unwrap();assert!(!json.contains("charge"));}
}
