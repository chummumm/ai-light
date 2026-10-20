//! Observe Windows connection and resume state without sending device commands.

#[cfg(windows)]
mod native {
    use anyhow::{bail, ensure, Context, Result};
    use std::{
        ffi::c_void,
        ptr,
        sync::atomic::{AtomicBool, Ordering},
    };
    use windows::{
        core::HRESULT,
        Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothLEDevice},
        Win32::{
            Foundation::{ERROR_SUCCESS, HANDLE},
            System::Power::{
                PowerRegisterSuspendResumeNotification, PowerUnregisterSuspendResumeNotification,
                DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY,
            },
            UI::WindowsAndMessaging::{DEVICE_NOTIFY_CALLBACK, PBT_APMRESUMEAUTOMATIC},
        },
    };

    #[link(name = "runtimeobject")]
    extern "system" {
        fn RoInitialize(kind: u32) -> i32;
        fn RoUninitialize();
    }

    struct Apartment;

    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }

    /// Read the OS connection property in the caller's bounded hardware worker.
    /// No GATT discovery/read, MaintainConnection, pairing or LED command is used.
    pub fn connection_status(serial: &str) -> Result<bool> {
        ensure!(
            serial.len() == 12 && serial.bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid Bluetooth serial"
        );
        let address = u64::from_str_radix(serial, 16).context("invalid Bluetooth address")?;
        // Both S_OK and S_FALSE require a matching RoUninitialize on this thread.
        unsafe { HRESULT(RoInitialize(1)).ok() }.context("initialize WinRT connection reader")?;
        let _apartment = Apartment;
        let device = BluetoothLEDevice::FromBluetoothAddressAsync(address)?
            .get()
            .context("open paired Bluetooth device for connection status")?;
        let status = device.ConnectionStatus();
        // Release only this WinRT object; do not change pairing or radio state.
        let _ = device.Close();
        match status.context("read Bluetooth connection status")? {
            BluetoothConnectionStatus::Connected => Ok(true),
            BluetoothConnectionStatus::Disconnected => Ok(false),
            other => bail!("unknown Bluetooth connection status: {}", other.0),
        }
    }

    // The hardware owner holds one monitor for the process lifetime. The callback
    // never dereferences a caller context, including while unregistering at exit.
    static MONITOR_REGISTERED: AtomicBool = AtomicBool::new(false);
    static RESUMED: AtomicBool = AtomicBool::new(false);

    unsafe extern "system" fn power_event(
        _context: *const c_void,
        event: u32,
        _setting: *const c_void,
    ) -> u32 {
        // Windows sends AUTOMATIC on resume, then may send RESUMESUSPEND when
        // the user interacts. Consuming both would restart the same light twice.
        if event == PBT_APMRESUMEAUTOMATIC {
            RESUMED.store(true, Ordering::Release);
        }
        ERROR_SUCCESS.0
    }

    pub struct ResumeMonitor {
        registration: HPOWERNOTIFY,
        parameters: Option<Box<DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS>>,
    }

    impl ResumeMonitor {
        pub fn new() -> Result<Self> {
            ensure!(
                MONITOR_REGISTERED
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok(),
                "a Windows resume monitor is already registered"
            );
            let mut parameters = Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
                Callback: Some(power_event),
                Context: ptr::null_mut(),
            });
            let mut registration = ptr::null_mut();
            RESUMED.store(false, Ordering::Release);
            // The subscription stays at a stable address until unregistration;
            // its callback and null context are valid for the whole process.
            let error = unsafe {
                PowerRegisterSuspendResumeNotification(
                    DEVICE_NOTIFY_CALLBACK,
                    HANDLE((&mut *parameters as *mut DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS).cast()),
                    &mut registration,
                )
            };
            if error != ERROR_SUCCESS {
                MONITOR_REGISTERED.store(false, Ordering::Release);
                return Err(std::io::Error::from_raw_os_error(error.0 as i32))
                    .context("register Windows suspend/resume notifications");
            }
            let registration = HPOWERNOTIFY(registration as isize);
            if registration.is_invalid() {
                MONITOR_REGISTERED.store(false, Ordering::Release);
                bail!("Windows returned an invalid suspend/resume registration");
            }
            Ok(Self {
                registration,
                parameters: Some(parameters),
            })
        }

        pub fn take_resumed(&self) -> bool {
            RESUMED.swap(false, Ordering::AcqRel)
        }
    }

    impl Drop for ResumeMonitor {
        fn drop(&mut self) {
            let error = unsafe { PowerUnregisterSuspendResumeNotification(self.registration) };
            if error == ERROR_SUCCESS {
                MONITOR_REGISTERED.store(false, Ordering::Release);
            } else if let Some(parameters) = self.parameters.take() {
                // An unexpectedly live registration must not retain a dangling
                // subscription pointer. Keep it valid until process termination.
                let _ = Box::leak(parameters);
            }
        }
    }
}

#[cfg(windows)]
pub use native::{connection_status, ResumeMonitor};

#[cfg(not(windows))]
pub fn connection_status(_: &str) -> anyhow::Result<bool> {
    anyhow::bail!("Windows Bluetooth connection reader only")
}

#[cfg(not(windows))]
pub struct ResumeMonitor;

#[cfg(not(windows))]
impl ResumeMonitor {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self)
    }

    pub fn take_resumed(&self) -> bool {
        false
    }
}
