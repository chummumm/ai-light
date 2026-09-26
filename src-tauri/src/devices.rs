//! Discovery is restricted to the supported vendor HID collection.
use anyhow::Result;
use serde::Serialize;
#[derive(Debug, Clone, Serialize)]
pub struct Device { pub serial: String, pub name: String }
#[cfg(windows)]
pub fn discover() -> Result<Vec<Device>> {
    use light_core::protocol::{VENDOR_ID, PRODUCT_ID, USAGE_PAGE, USAGE};
    let api = hidapi::HidApi::new()?;
    let mut result = std::collections::BTreeMap::new();
    for d in api.device_list() {
        if d.vendor_id()!=VENDOR_ID || d.product_id()!=PRODUCT_ID || d.usage_page()!=USAGE_PAGE || d.usage()!=USAGE {continue;}
        if let Some(serial)=d.serial_number().filter(|s|s.len()==12 && s.bytes().all(|b|b.is_ascii_hexdigit())) {
            let serial=serial.to_ascii_lowercase();
            result.insert(serial.clone(),Device{serial,name:d.product_string().unwrap_or("Compatible AI Light").to_owned()});
        }
    }
    Ok(result.into_values().collect())
}
#[cfg(not(windows))]
pub fn discover() -> Result<Vec<Device>> {anyhow::bail!("Device discovery requires Windows")}
