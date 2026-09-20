use std::fmt;
use std::net::IpAddr;
use std::path::{Component, Path, PathBuf};

use idevice::core_device_proxy::CoreDeviceProxy;
use idevice::installation_proxy::InstallationProxyClient;
use idevice::lockdown::LockdownClient;
use idevice::misagent::MisagentClient;
use idevice::pairing_file::PairingFile;
use idevice::provider::{IdeviceProvider, TcpProvider, UsbmuxdProvider};
use idevice::remote_pairing::{RemotePairingClient, RpPairingFile};
use idevice::rsd::RsdHandshake;
use idevice::usbmuxd::{Connection, UsbmuxdAddr, UsbmuxdDevice};
use idevice::utils::installation;
use idevice::{IdeviceService, RemoteXpcClient};
use plume_core::MobileProvision;

use crate::Error;
use crate::options::SignerAppReal;
use idevice::afc::opcode::AfcFopenMode;
use idevice::house_arrest::HouseArrestClient;
use idevice::usbmuxd::UsbmuxdConnection;
use plist::Value;

pub const CONNECTION_LABEL: &str = "plume_info";
pub const INSTALLATION_LABEL: &str = "plume_install";
pub const HOUSE_ARREST_LABEL: &str = "plume_house_arrest";

macro_rules! get_dict_string {
    ($dict:expr, $key:expr) => {
        $dict
            .as_dictionary()
            .and_then(|dict| dict.get($key))
            .and_then(|v| v.as_string())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "".to_string())
    };
}

#[derive(Debug, Clone)]
pub struct Device {
    pub name: String,
    pub udid: String,
    pub device_id: u32,
    pub usbmuxd_device: Option<UsbmuxdDevice>,
    /// Set when the phone was found through Wi-Fi instead of through Apple's
    /// device service: all communication then goes straight to this address.
    pub network_address: Option<IpAddr>,
    pairing_file: Option<PairingFile>,
    // On x86_64 macs, `is_mac` variable should never be true
    // since its only true if the device is added manually.
    pub is_mac: bool,
}

impl Device {
    pub async fn new(usbmuxd_device: UsbmuxdDevice) -> Self {
        let name = Self::get_name_from_usbmuxd_device(&usbmuxd_device)
            .await
            .unwrap_or_default();

        Device {
            name,
            udid: usbmuxd_device.udid.clone(),
            device_id: usbmuxd_device.device_id.clone(),
            usbmuxd_device: Some(usbmuxd_device),
            network_address: None,
            pairing_file: None,
            is_mac: false,
        }
    }

    /// Finds a paired phone on the local network and prepares a direct Wi-Fi
    /// connection to it.
    ///
    /// Apple's Windows service only reports phones that it discovered itself,
    /// and that discovery is not always running. Talking to the phone directly
    /// (the address comes from Apple's own mDNS advertisement) is what makes
    /// renewals work after the cable is unplugged.
    pub async fn connect_over_network(udid: &str) -> Result<Device, Error> {
        Self::connect_over_network_with_mac(udid, None).await
    }

    /// Like [`Device::connect_over_network`], but also accepts the phone's
    /// Wi-Fi MAC address (learned during a USB session) which allows finding it
    /// through the ARP table when mDNS is filtered by the network.
    pub async fn connect_over_network_with_mac(
        udid: &str,
        wifi_mac: Option<&str>,
    ) -> Result<Device, Error> {
        let mut candidates: Vec<IpAddr> = Vec::new();

        if let Some(address) =
            crate::network::discover_address(udid, std::time::Duration::from_secs(5))
        {
            candidates.push(address);
        }

        // Apple's advert does not always carry the UDID (and the phone may use
        // a private Wi-Fi address, so the name cannot be matched either), so
        // treat every Apple device on this network as a candidate and verify
        // the identity after connecting.
        for address in crate::network::discover_all_addresses(std::time::Duration::from_secs(5)) {
            candidates.push(address);
        }

        if let Some(mac) = wifi_mac {
            if let Some(address) = crate::network::arp_address_for_mac(mac) {
                log::info!("{udid} is in the ARP table at {address}");
                candidates.push(address);
            }
        }

        if candidates.is_empty() {
            log::info!(
                "Neither mDNS nor ARP found {udid}; scanning the local network for a phone (port 62078)..."
            );
            let found =
                crate::network::scan_local_subnet(62078, std::time::Duration::from_millis(700))
                    .await;
            if !found.is_empty() {
                log::info!("Network scan found {} candidate(s): {found:?}", found.len());
            }
            candidates.extend(found);
        }

        candidates.retain(crate::network::is_private_lan);
        candidates.dedup();

        let mut last_error = None;

        for address in candidates {
            match Self::try_network_device(udid, address).await {
                Ok(device) => return Ok(device),
                Err(err) => {
                    log::warn!("{address} does not look like {udid}: {err}");
                    last_error = Some(err);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            Error::Other(format!(
                "{udid} was not found on the local network (mDNS, ARP and a port scan all came up empty)"
            ))
        }))
    }

    /// Opens a direct Wi-Fi connection to `address` and confirms that the phone
    /// on the other end really is `udid`.
    async fn try_network_device(udid: &str, address: IpAddr) -> Result<Device, Error> {
        let mut usbmuxd = UsbmuxdConnection::default().await?;
        let pairing_file = usbmuxd.get_pair_record(udid).await?;

        let device = Device {
            name: String::new(),
            udid: udid.to_string(),
            device_id: 0,
            usbmuxd_device: None,
            network_address: Some(address),
            pairing_file: Some(pairing_file),
            is_mac: false,
        };

        let provider = device.provider(CONNECTION_LABEL)?;
        let mut lockdown = LockdownClient::connect(provider.as_ref()).await?;
        let values = lockdown.get_value(None, None).await?;

        let reported = get_dict_string!(values, "UniqueDeviceID");
        if !reported.is_empty() && !same_udid(&reported, udid) {
            return Err(Error::Other(format!(
                "device at {address} reports UDID {reported}"
            )));
        }

        Ok(Device {
            name: get_dict_string!(values, "DeviceName"),
            ..device
        })
    }

    /// The phone's Wi-Fi hardware address, read while it is connected over USB.
    ///
    /// Remembering it lets AutoSign find the phone through the ARP table later,
    /// which keeps wireless renewals working on networks that filter mDNS.
    pub async fn wifi_mac(&self) -> Option<String> {
        let provider = self.provider(CONNECTION_LABEL).ok()?;
        let mut lockdown = LockdownClient::connect(provider.as_ref()).await.ok()?;
        let values = lockdown.get_value(None, None).await.ok()?;

        let mac = get_dict_string!(values, "WiFiAddress");
        (!mac.is_empty()).then_some(mac)
    }

    /// Provider for whichever transport this device is reachable through:
    /// Apple's device service for USB (and discovered Wi-Fi) devices, or a
    /// direct TCP connection for phones found on the local network.
    fn provider(&self, label: &str) -> Result<Box<dyn IdeviceProvider>, Error> {
        if let Some(device) = &self.usbmuxd_device {
            return Ok(Box::new(device.to_provider(
                UsbmuxdAddr::from_env_var().unwrap_or_default(),
                label,
            )));
        }

        if let (Some(address), Some(pairing_file)) = (self.network_address, &self.pairing_file) {
            return Ok(Box::new(TcpProvider {
                addr: address,
                scope_id: None,
                pairing_file: pairing_file.clone(),
                label: label.to_string(),
            }));
        }

        Err(Error::Other("Device is not connected".to_string()))
    }

    async fn get_name_from_usbmuxd_device(device: &UsbmuxdDevice) -> Result<String, Error> {
        let mut lockdown =
            LockdownClient::connect(&device.to_provider(UsbmuxdAddr::default(), CONNECTION_LABEL))
                .await?;
        let values = lockdown.get_value(None, None).await?;
        Ok(get_dict_string!(values, "DeviceName"))
    }

    pub async fn installed_apps(&self) -> Result<Vec<SignerAppReal>, Error> {
        let provider = self.provider(INSTALLATION_LABEL)?;

        let mut ic = InstallationProxyClient::connect(provider.as_ref()).await?;
        let apps = ic.get_apps(Some("User"), None).await?;

        let mut found_apps = Vec::new();

        for (bundle_id, info) in apps {
            let app_name = get_app_name_from_info(&info);
            let signer_app = SignerAppReal::from_bundle_identifier_and_name(
                Some(bundle_id.as_str()),
                app_name.as_deref(),
            );

            if signer_app.app.supports_pairing_file_alt()
                && !found_apps
                    .iter()
                    .any(|a: &SignerAppReal| a.bundle_id == signer_app.bundle_id)
            {
                found_apps.push(signer_app);
            }
        }

        Ok(found_apps)
    }

    pub async fn is_app_installed(&self, bundle_id: &str) -> Result<bool, Error> {
        let provider = self.provider(INSTALLATION_LABEL)?;

        let mut ic = InstallationProxyClient::connect(provider.as_ref()).await?;
        let apps = ic.get_apps(Some("User"), None).await?;

        Ok(apps.contains_key(bundle_id))
    }

    pub async fn install_profile(&self, profile: &MobileProvision) -> Result<(), Error> {
        let provider = self.provider(INSTALLATION_LABEL)?;

        let mut mc = MisagentClient::connect(provider.as_ref()).await?;
        mc.install(profile.data.clone()).await?;

        Ok(())
    }

    pub async fn pair(&self) -> Result<(), Error> {
        if self.usbmuxd_device.is_none() {
            return Err(Error::Other("Device is not connected via USB".to_string()));
        }

        let mut usbmuxd = UsbmuxdConnection::default().await?;

        let provider = self.provider(INSTALLATION_LABEL)?;

        let mut lc = LockdownClient::connect(provider.as_ref()).await?;
        let id = uuid::Uuid::new_v4().to_string().to_uppercase();
        let buid = usbmuxd.get_buid().await?;
        let mut pairing_file = lc.pair(id, buid, None).await?;
        pairing_file.udid = Some(self.udid.clone());
        let pairing_file = pairing_file.serialize()?;

        usbmuxd.save_pair_record(&self.udid, pairing_file).await?;

        Ok(())
    }

    pub async fn install_pairing_record(
        &self,
        identifier: &String,
        path: &str,
    ) -> Result<(), Error> {
        let mut usbmuxd = UsbmuxdConnection::default().await?;
        let provider = self.provider(HOUSE_ARREST_LABEL)?;
        let mut pairing_file = usbmuxd.get_pair_record(&self.udid).await?;

        // saving pairing record requires enabling wifi debugging
        // since operations are done over wifi
        let mut lc = LockdownClient::connect(provider.as_ref()).await?;
        lc.start_session(&pairing_file).await.ok();
        lc.set_value(
            "EnableWifiDebugging",
            true.into(),
            Some("com.apple.mobile.wireless_lockdown"),
        )
        .await
        .ok();

        pairing_file.udid = Some(self.udid.clone());

        let hc = HouseArrestClient::connect(provider.as_ref()).await?;
        let mut ac = hc.vend_documents(identifier.clone()).await?;
        if let Some(parent) = Path::new(path).parent() {
            let mut current = String::new();
            let has_root = parent.has_root();

            for component in parent.components() {
                if let Component::Normal(dir) = component {
                    if has_root && current.is_empty() {
                        current.push('/');
                    } else if !current.is_empty() && !current.ends_with('/') {
                        current.push('/');
                    }

                    current.push_str(&dir.to_string_lossy());
                    ac.mk_dir(&current).await?;
                }
            }
        }

        let mut f = ac.open(path, AfcFopenMode::Wr).await?;
        f.write_entire(&pairing_file.serialize().unwrap()).await?;

        Ok(())
    }

    /// True when this device is reached over the local network (Wi-Fi) rather
    /// than over a USB cable.
    pub fn is_network(&self) -> bool {
        self.network_address.is_some()
            || matches!(
                self.usbmuxd_device.as_ref().map(|d| &d.connection_type),
                Some(Connection::Network(_))
            )
    }

    /// Ask the device to also accept lockdown connections over Wi-Fi.
    ///
    /// iOS only honours wireless connections for computers which have been
    /// trusted while plugged in, so this is called right after a USB connection
    /// is seen. Enabling it is what later allows automatic renewals while the
    /// phone is only sitting on the same Wi-Fi network as this computer.
    pub async fn enable_wireless_connections(&self) -> Result<(), Error> {
        let Some(device) = &self.usbmuxd_device else {
            return Err(Error::Other("Device is not connected via USB".to_string()));
        };

        let mut usbmuxd = UsbmuxdConnection::default().await?;
        let pairing_file = usbmuxd.get_pair_record(&self.udid).await?;

        let provider = device.to_provider(UsbmuxdAddr::default(), CONNECTION_LABEL);
        let mut lc = LockdownClient::connect(&provider).await?;
        lc.start_session(&pairing_file).await.ok();

        lc.set_value(
            "EnableWifiConnections",
            true.into(),
            Some("com.apple.mobile.wireless_lockdown"),
        )
        .await?;

        // Wireless debugging is separate from wireless sync, but enabling both
        // keeps the device reachable for every service we use.
        lc.set_value(
            "EnableWifiDebugging",
            true.into(),
            Some("com.apple.mobile.wireless_lockdown"),
        )
        .await
        .ok();

        Ok(())
    }

    /// Reads back what the phone actually stored for wireless sync.
    ///
    /// iOS silently ignores some writes, so this is logged after enabling the
    /// setting: it is the difference between "we asked for Wi-Fi" and "the phone
    /// is really advertising itself".
    pub async fn wireless_state(&self) -> Option<String> {
        let provider = self.provider(CONNECTION_LABEL).ok()?;
        let mut lockdown = LockdownClient::connect(provider.as_ref()).await.ok()?;

        let usbmuxd = UsbmuxdConnection::default().await.ok();
        if let Some(mut usbmuxd) = usbmuxd {
            if let Ok(pairing_file) = usbmuxd.get_pair_record(&self.udid).await {
                lockdown.start_session(&pairing_file).await.ok();
            }
        }

        let values = lockdown
            .get_value(Some("com.apple.mobile.wireless_lockdown"), None)
            .await
            .ok()?;

        Some(format!("{values:?}"))
    }

    /// Fully prepares the phone for wireless use.
    ///
    /// * pairs the phone when there is no pairing record yet - iOS then shows
    ///   its "Trust This Computer?" prompt, which is the one thing only the
    ///   user can confirm - and
    /// * switches wireless sync on, so the phone starts advertising itself on
    ///   the local network and AutoSign can reach it after unplugging.
    pub async fn prepare_wireless(&self) -> Result<(), Error> {
        let mut usbmuxd = UsbmuxdConnection::default().await?;

        if usbmuxd.get_pair_record(&self.udid).await.is_err() {
            log::info!(
                "No pairing record for {} yet; pairing now (the phone will ask to trust this computer)",
                self.udid
            );
            self.pair().await?;
        }

        self.enable_wireless_connections().await?;

        // iOS ignores some writes without telling us, so read the setting back.
        match self.wireless_state().await {
            Some(state) => log::info!("Phone wireless settings: {state}"),
            None => log::warn!("Could not read back the phone's wireless settings"),
        }

        Ok(())
    }

    /// Confirms that the phone can be reached over Wi-Fi and returns how.
    pub async fn verify_wireless(&self) -> Result<IpAddr, Error> {
        let wireless = Device::connect_over_network(&self.udid).await?;

        wireless
            .network_address
            .ok_or_else(|| Error::Other("no wireless address".to_string()))
    }

    pub async fn install_remote_pairing_record(
        &self,
        identifier: &String,
        path: &str,
        path_to_store: PathBuf,
    ) -> Result<(), Error> {
        let provider = self.provider(HOUSE_ARREST_LABEL)?;

        let pairing_file = self
            .get_rsd_pairing_file(provider.as_ref(), path_to_store)
            .await?;

        let hc = HouseArrestClient::connect(provider.as_ref()).await?;
        let mut ac = hc.vend_documents(identifier.clone()).await?;
        if let Some(parent) = Path::new(path).parent() {
            let mut current = String::new();
            let has_root = parent.has_root();

            for component in parent.components() {
                if let Component::Normal(dir) = component {
                    if has_root && current.is_empty() {
                        current.push('/');
                    } else if !current.is_empty() && !current.ends_with('/') {
                        current.push('/');
                    }

                    current.push_str(&dir.to_string_lossy());
                    ac.mk_dir(&current).await?;
                }
            }
        }

        let mut f = ac.open(path, AfcFopenMode::Wr).await?;
        f.write_entire(&pairing_file.to_bytes()).await?;

        Ok(())
    }

    async fn get_rsd_pairing_file(
        &self,
        provider: &dyn IdeviceProvider,
        path: PathBuf,
    ) -> Result<RpPairingFile, Error> {
        let pairing_file_path = path.join(format!("plume_{}.plist", self.udid));

        if pairing_file_path.exists() {
            return Ok(RpPairingFile::read_from_file(pairing_file_path).await?);
        } else {
            let cdp = CoreDeviceProxy::connect(provider).await?;
            let cdp_port = cdp.tunnel_info().server_rsd_port;
            let cdp_adapter = cdp.create_software_tunnel()?;
            let mut cdp_adapter = cdp_adapter.to_async_handle();

            let cdp_stream = cdp_adapter.connect(cdp_port).await?;
            let cdp_handshake = RsdHandshake::new(cdp_stream).await?;

            let tunnel_service = cdp_handshake
                .services
                .get("com.apple.internal.dt.coredevice.untrusted.tunnelservice")
                .ok_or_else(|| Error::Other("Tunnel service not found".to_string()))?;

            let tunnel_service_stream = cdp_adapter.connect(tunnel_service.port).await?;
            let mut remote_xpc = RemoteXpcClient::new(tunnel_service_stream).await?;
            remote_xpc.do_handshake().await?;
            let _ = remote_xpc.recv_root().await;

            let suffix: String = uuid::Uuid::new_v4()
                .simple()
                .to_string()
                .chars()
                .take(6)
                .collect();

            let hostname = format!("plume-{}", suffix);

            let mut pairing_file = RpPairingFile::generate(&hostname);
            let mut pairing_client =
                RemotePairingClient::new(remote_xpc, &hostname, &mut pairing_file);
            pairing_client
                .connect(async |_| "000000".to_string(), ())
                .await?;

            let tunnel_service_stream = cdp_adapter.connect(tunnel_service.port).await?;
            let mut remote_xpc = RemoteXpcClient::new(tunnel_service_stream).await?;
            remote_xpc.do_handshake().await?;
            let _ = remote_xpc.recv_root().await;
            let mut pairing_client =
                RemotePairingClient::new(remote_xpc, &hostname, &mut pairing_file);
            pairing_client
                .connect(async |_| "000000".to_string(), ())
                .await?;

            let pairing_file_bytes = pairing_file.to_bytes();

            tokio::fs::write(&pairing_file_path, &pairing_file_bytes).await?;

            Ok(pairing_file)
        }
    }

    pub async fn install_app<F, Fut>(
        &self,
        app_path: &PathBuf,
        progress_callback: F,
    ) -> Result<(), Error>
    where
        F: FnMut(i32) -> Fut + Send + Clone + 'static,
        Fut: std::future::Future<Output = ()> + Send,
    {
        let provider = self.provider(INSTALLATION_LABEL)?;

        let callback = move |(progress, _): (u64, ())| {
            let mut cb = progress_callback.clone();
            async move {
                cb(progress as i32).await;
            }
        };

        let state = ();

        installation::install_package_with_callback(
            provider.as_ref(),
            app_path,
            None,
            callback,
            state,
        )
        .await?;

        Ok(())
    }
}

fn get_app_name_from_info(info: &Value) -> Option<String> {
    let dict = info.as_dictionary()?;
    dict.get("CFBundleDisplayName")
        .and_then(|value| value.as_string())
        .or_else(|| dict.get("CFBundleName").and_then(|value| value.as_string()))
        .or_else(|| {
            dict.get("CFBundleExecutable")
                .and_then(|value| value.as_string())
        })
        .map(|value| value.to_string())
}

/// UDIDs are compared without separators and case differences.
fn same_udid(left: &str, right: &str) -> bool {
    let normalize = |value: &str| -> String {
        value
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_lowercase()
    };

    normalize(left) == normalize(right)
}

impl fmt::Display for Device {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {}",
            match &self.usbmuxd_device {
                Some(device) => match &device.connection_type {
                    Connection::Usb => "USB",
                    Connection::Network(_) => "WiFi",
                    Connection::Unknown(_) => "Unknown",
                },
                None => "LOCAL",
            },
            self.name
        )
    }
}

pub async fn get_device_for_id(device_id: &str) -> Result<Device, Error> {
    let mut usbmuxd = UsbmuxdConnection::default().await?;
    let usbmuxd_device = usbmuxd
        .get_devices()
        .await?
        .into_iter()
        .find(|d| d.device_id.to_string() == device_id)
        .ok_or_else(|| Error::Other(format!("Device ID {device_id} not found")))?;

    Ok(Device::new(usbmuxd_device).await)
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub async fn install_app_mac(app_path: &PathBuf) -> Result<(), Error> {
    use crate::copy_dir_recursively;
    use std::env;
    use tokio::fs;
    use uuid::Uuid;

    let stage_dir = env::temp_dir().join(format!(
        "plume_mac_stage_{}",
        Uuid::new_v4().to_string().to_uppercase()
    ));
    let app_name = app_path
        .file_name()
        .ok_or(Error::Other("Invalid app path".to_string()))?;

    // iOS Apps on macOS need to be wrapped in a special structure, more specifically
    // ```
    // LiveContainer.app
    // ├── WrappedBundle -> Wrapper/LiveContainer.app
    // └── Wrapper
    //     └── LiveContainer.app
    // ```
    // Then install to /Applications/...

    let outer_app_dir = stage_dir.join(app_name);
    let wrapper_dir = outer_app_dir.join("Wrapper");

    fs::create_dir_all(&wrapper_dir).await?;

    copy_dir_recursively(app_path, &wrapper_dir.join(app_name)).await?;

    let wrapped_bundle_path = outer_app_dir.join("WrappedBundle");
    fs::symlink(
        PathBuf::from("Wrapper").join(app_name),
        &wrapped_bundle_path,
    )
    .await?;

    let applications_dir = PathBuf::from("/Applications/iOS");
    fs::create_dir_all(&applications_dir).await?;

    let applications_dir = applications_dir.join(app_name);

    fs::remove_dir_all(&applications_dir).await.ok();

    fs::rename(&outer_app_dir, &applications_dir)
        .await
        .map_err(|_| Error::BundleFailedToCopy(applications_dir.to_string_lossy().into_owned()))?;

    Ok(())
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub async fn install_app_mac(_app_path: &PathBuf) -> Result<(), Error> {
    Ok(())
}
