//! WinRT GATT battery reads run only in a bounded child process.
#[cfg(windows)]
pub fn read(serial:&str)->anyhow::Result<light_core::battery::BatteryReading> {
    use anyhow::{ensure,Context,Result};
    use light_core::battery::{BatteryReading,parse_level};
    use windows::{core::GUID,Devices::Bluetooth::{BluetoothLEDevice,BluetoothCacheMode},
        Devices::Bluetooth::GenericAttributeProfile::{GattCharacteristic,GattCommunicationStatus},
        Storage::Streams::DataReader};
    #[link(name="runtimeobject")]
    extern "system" {fn RoInitialize(kind:u32)->i32;fn RoUninitialize();}
    struct Apartment;
    impl Drop for Apartment {fn drop(&mut self){unsafe{RoUninitialize();}}}
    ensure!(unsafe{RoInitialize(1)}>=0,"WinRT MTA initialization failed");
    let _apartment=Apartment;
    let address=u64::from_str_radix(serial,16).context("invalid Bluetooth address")?;
    let device=BluetoothLEDevice::FromBluetoothAddressAsync(address)?.get()
        .context("Windows could not open the paired Bluetooth device")?;
    fn guid(id:u16)->GUID {GUID::from_u128(((id as u128)<<96)|0x0000_1000_8000_00805f9b34fb)}
    fn read_bytes(c:&GattCharacteristic)->Result<Vec<u8>> {
        let r=c.ReadValueWithCacheModeAsync(BluetoothCacheMode::Uncached)?.get()?;
        ensure!(r.Status()?==GattCommunicationStatus::Success,"GATT read status: {:?}",r.Status()?);
        let value=r.Value()?;
        ensure!(value.Length()?<=128,"battery value is unexpectedly large");
        let mut bytes=vec![0;value.Length()? as usize];
        let reader=DataReader::FromBuffer(&value)?;
        reader.ReadBytes(&mut bytes)?;
        Ok(bytes)
    }
    let result=(||->Result<BatteryReading>{
        let services_result=device.GetGattServicesForUuidWithCacheModeAsync(guid(0x180f),BluetoothCacheMode::Uncached)?.get()?;
        ensure!(services_result.Status()?==GattCommunicationStatus::Success,"Battery Service access: {:?}",services_result.Status()?);
        let services=services_result.Services()?;
        ensure!(services.Size()?>0,"device does not expose Battery Service 0x180F");
        ensure!(services.Size()?==1,"multiple Battery Services; primary battery selection required");
        let service=services.GetAt(0)?;
        let result=(||->Result<BatteryReading>{
            let r=service.GetCharacteristicsWithCacheModeAsync(BluetoothCacheMode::Uncached)?.get()?;
            ensure!(r.Status()?==GattCommunicationStatus::Success,"battery characteristic discovery: {:?}",r.Status()?);
            let chars=r.Characteristics()?;
            let mut out=BatteryReading::default();
            let mut level=None;let mut notes=Vec::new();let mut attempted=false;let mut read_ok=false;let mut uuids=Vec::new();
            for i in 0..chars.Size()? {
                let c=chars.GetAt(i)?;let id=c.Uuid()?;
                uuids.push(format!("{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",id.data1,id.data2,id.data3,id.data4[0],id.data4[1],id.data4[2],id.data4[3],id.data4[4],id.data4[5],id.data4[6],id.data4[7]));
                if id==guid(0x2a19) {
                    attempted=true;
                    match read_bytes(&c).and_then(|v|parse_level(&v)){Ok(p)=>{level=Some(p);read_ok=true;},Err(e)=>notes.push(format!("0x2A19: {e}"))}
                }
            }
            ensure!(attempted&&read_ok,"Battery read failed: {}",notes.join("; "));
            if level.is_some(){out.percent=level;}
            if out.source.is_empty(){out.source=if level.is_some(){"BLE Battery Level · 0x2A19"}else{"BLE Battery Service · 0x180F"}.into();}
            if out.percent.is_none(){notes.push("设备未返回有效电量百分比".into());}
            out.note=notes.join("；");out.characteristics=uuids;
            Ok(out)
        })();
        let _=service.Close();result
    })();
    let _=device.Close();result
}
#[cfg(not(windows))]
pub fn read(_: &str)->anyhow::Result<light_core::battery::BatteryReading>{anyhow::bail!("Windows battery reader only")}
