// SPDX-License-Identifier: Apache-2.0

use super::asio_import::ASIODriverInfo;

/// ASIOInit reads the host version and window before writing driver metadata.
pub(super) fn initialized() -> ASIODriverInfo {
    #[cfg(windows)]
    // SAFETY: GetDesktopWindow takes no arguments and returns a borrowed HWND valid
    // for the desktop's lifetime. ASIO uses it only as an owner for driver dialogs.
    let window = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetDesktopWindow() };
    #[cfg(not(windows))]
    let window = std::ptr::null_mut();

    ASIODriverInfo {
        asioVersion: 2,
        driverVersion: 0,
        name: [0; 32],
        errorMessage: [0; 124],
        sysRef: window,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn initialization_supplies_host_inputs_and_empty_output_fields() {
        let info = super::initialized();
        assert_eq!(info.asioVersion, 2);
        assert_eq!(info.driverVersion, 0);
        assert!(info.name.iter().all(|byte| *byte == 0));
        assert!(info.errorMessage.iter().all(|byte| *byte == 0));
        #[cfg(windows)]
        assert!(!info.sysRef.is_null());
        #[cfg(not(windows))]
        assert!(info.sysRef.is_null());
    }
}
