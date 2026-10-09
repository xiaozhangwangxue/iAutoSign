use crate::anisette_headers_provider::AnisetteHeadersProvider;
use crate::AnisetteError;
use anyhow::Result;

use dlopen2::symbor::Library;
use objc::{msg_send, runtime::Class, sel, sel_impl};
use objc_foundation::{INSString, NSObject, NSString};
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Local anisette data straight from Apple's private `AOSKit` framework.
///
/// The class pointers are kept as `usize` so that the provider (and therefore
/// the futures built from it) stay `Send`.
pub struct AOSKitAnisetteProvider {
    aos_utilities: usize,
    ak_device: usize,
}

impl AOSKitAnisetteProvider {
    pub fn new() -> Result<AOSKitAnisetteProvider> {
        Library::open("/System/Library/PrivateFrameworks/AOSKit.framework/AOSKit")?;
        Library::open("/System/Library/PrivateFrameworks/AuthKit.framework/AuthKit")?;

        let aos_utilities = Class::get("AOSUtilities").ok_or(AOSKitError::ClassLoadFailed)?;
        let ak_device = Class::get("AKDevice").ok_or(AOSKitError::ClassLoadFailed)?;

        Ok(AOSKitAnisetteProvider {
            aos_utilities: aos_utilities as *const Class as usize,
            ak_device: ak_device as *const Class as usize,
        })
    }

    fn aos_utilities(&self) -> &Class {
        unsafe { &*(self.aos_utilities as *const Class) }
    }

    fn ak_device(&self) -> &Class {
        unsafe { &*(self.ak_device as *const Class) }
    }

    /// Reads an `NSString` value, treating a missing (nil) value as an error
    /// instead of dereferencing it.
    fn read_string(object: *const NSString, field: &str) -> Result<String, AnisetteError> {
        if object.is_null() {
            return Err(AnisetteError::Anyhow(anyhow::anyhow!(
                "AOSKit did not return {field} (Apple removed anisette data on macOS 27)"
            )));
        }

        Ok(unsafe { (*object).as_str() }.to_string())
    }
}

#[cfg_attr(feature = "async", async_trait::async_trait)]
impl AnisetteHeadersProvider for AOSKitAnisetteProvider {
    #[cfg_attr(not(feature = "async"), remove_async_await::remove_async_await)]
    async fn get_anisette_headers(
        &mut self,
        _skip_provisioning: bool,
    ) -> Result<HashMap<String, String>, AnisetteError> {
        let mut headers_map = HashMap::new();

        let headers: *const NSObject = unsafe {
            msg_send![self.aos_utilities(), retrieveOTPHeadersForDSID: NSString::from_str("-2")]
        };

        if headers.is_null() {
            return Err(AnisetteError::Anyhow(anyhow::anyhow!(
                "AOSKit returned no anisette headers (macOS 27 no longer provides them)"
            )));
        }

        let otp: *const NSString =
            unsafe { msg_send![headers, valueForKey: NSString::from_str("X-Apple-MD")] };
        headers_map.insert(
            "X-Apple-I-MD".to_string(),
            Self::read_string(otp, "X-Apple-MD")?,
        );

        let mid: *const NSString =
            unsafe { msg_send![headers, valueForKey: NSString::from_str("X-Apple-MD-M")] };
        headers_map.insert(
            "X-Apple-I-MD-M".to_string(),
            Self::read_string(mid, "X-Apple-MD-M")?,
        );

        let machine_serial_number: *const NSString =
            unsafe { msg_send![self.aos_utilities(), machineSerialNumber] };
        headers_map.insert(
            "X-Apple-SRL-NO".to_string(),
            Self::read_string(machine_serial_number, "machineSerialNumber")?,
        );

        let current_device: *const NSObject =
            unsafe { msg_send![self.ak_device(), currentDevice] };

        let local_user_uuid: *const NSString = unsafe { msg_send![current_device, localUserUUID] };
        headers_map.insert(
            "X-Apple-I-MD-LU".to_string(),
            Self::read_string(local_user_uuid, "localUserUUID")?,
        );

        let locale: *const NSObject = unsafe { msg_send![current_device, locale] };
        let locale: *const NSString = unsafe { msg_send![locale, localeIdentifier] };
        headers_map.insert(
            "X-Apple-Locale".to_string(),
            Self::read_string(locale, "locale")?,
        ); // FIXME maybe not the right header name

        let server_friendly_description: *const NSString =
            unsafe { msg_send![current_device, serverFriendlyDescription] };
        headers_map.insert(
            "X-Mme-Client-Info".to_string(),
            Self::read_string(server_friendly_description, "serverFriendlyDescription")?,
        );

        let unique_device_identifier: *const NSString =
            unsafe { msg_send![current_device, uniqueDeviceIdentifier] };
        headers_map.insert(
            "X-Mme-Device-Id".to_string(),
            Self::read_string(unique_device_identifier, "uniqueDeviceIdentifier")?,
        );

        Ok(headers_map)
    }
}

#[derive(Debug)]
enum AOSKitError {
    ClassLoadFailed,
}

impl Display for AOSKitError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Error for AOSKitError {}

#[cfg(all(test, not(feature = "async")))]
mod tests {
    use crate::anisette_headers_provider::AnisetteHeadersProvider;
    use crate::aos_kit::AOSKitAnisetteProvider;
    use anyhow::Result;
    use log::info;

    #[test]
    fn fetch_anisette_aoskit() -> Result<()> {
        crate::tests::init_logger();

        let mut provider = AOSKitAnisetteProvider::new()?;
        info!(
            "AOSKit headers: {:?}",
            (&mut provider as &mut dyn AnisetteHeadersProvider).get_authentication_headers()?
        );
        Ok(())
    }
}
