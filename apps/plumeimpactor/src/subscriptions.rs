use iced::{Subscription, window};
use idevice::usbmuxd::{UsbmuxdConnection, UsbmuxdListenEvent};
use rust_i18n::t;
use std::sync::Arc;
use std::time::Duration;
use tray_icon::{TrayIconEvent, menu::MenuEvent};

use crate::{
    defaults::get_data_path,
    screen::{Message, general},
};
use plume_utils::{Bundle, Device, PlistInfoTrait};

pub(crate) fn device_listener() -> Subscription<Message> {
    Subscription::run(|| {
        iced::stream::channel(
            100,
            |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                use iced::futures::{SinkExt, StreamExt};
                let (tx, mut rx) = iced::futures::channel::mpsc::unbounded::<Message>();

                // Apple's device service sometimes stops advertising a phone
                // after it has been unplugged. Reconnect directly using the
                // UDID and Wi-Fi address remembered during the USB pairing.
                let network_tx = tx.clone();
                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();
                    rt.block_on(async move {
                        loop {
                            let directory = get_data_path().join("wifi_macs");
                            if let Ok(entries) = std::fs::read_dir(directory) {
                                for entry in entries.filter_map(Result::ok) {
                                    let path = entry.path();
                                    let Some(udid) = path
                                        .file_stem()
                                        .and_then(|value| value.to_str())
                                        .map(str::to_string)
                                    else {
                                        continue;
                                    };
                                    let wifi_mac = std::fs::read_to_string(&path).ok();
                                    if let Ok(device) = Device::connect_over_network_with_mac(
                                        &udid,
                                        wifi_mac.as_deref().map(str::trim),
                                    )
                                    .await
                                    {
                                        let _ = network_tx
                                            .unbounded_send(Message::DeviceConnected(device));
                                    }
                                }
                            }
                            tokio::time::sleep(Duration::from_secs(30)).await;
                        }
                    });
                });

                std::thread::spawn(move || {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();

                    rt.block_on(async move {
                        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
                        {
                            if let Some(mac_udid) = plume_gestalt::get_udid() {
                                let _ = tx.unbounded_send(Message::DeviceConnected(
                                    Device::local_mac("This Mac", mac_udid, u32::MAX),
                                ));
                            }
                        }

                        // When AutoSign is started automatically at boot the
                        // Apple driver service may not be up yet, and the user
                        // would then never see their phone. Keep retrying
                        // instead of giving up on the device listener.
                        loop {
                            let mut muxer = loop {
                                match UsbmuxdConnection::default().await {
                                    Ok(muxer) => break muxer,
                                    Err(err) => {
                                        log::warn!(
                                            "Apple's device service is not reachable yet ({err}); retrying in 5 seconds"
                                        );
                                        tokio::time::sleep(Duration::from_secs(5)).await;
                                    }
                                }
                            };

                            if let Ok(devices) = muxer.get_devices().await {
                                for dev in devices {
                                    let device = Device::new(dev).await;
                                    prepare_wireless_refresh(device.clone());
                                    let _ = tx.unbounded_send(Message::DeviceConnected(device));
                                }
                            }

                            let Ok(mut stream) = muxer.listen().await else {
                                log::warn!(
                                    "Could not listen for devices; retrying in 5 seconds"
                                );
                                tokio::time::sleep(Duration::from_secs(5)).await;
                                continue;
                            };

                            while let Some(event) = stream.next().await {
                                let msg = match event {
                                    Ok(UsbmuxdListenEvent::Connected(dev)) => {
                                        let device = Device::new(dev).await;
                                        prepare_wireless_refresh(device.clone());
                                        Message::DeviceConnected(device)
                                    }
                                    Ok(UsbmuxdListenEvent::Disconnected(id)) => {
                                        Message::DeviceDisconnected(id)
                                    }
                                    Err(_) => continue,
                                };
                                let _ = tx.unbounded_send(msg);
                            }

                            // The service restarted (for example after an iTunes
                            // update); reconnect instead of going deaf.
                            log::warn!("Device listener stopped; reconnecting in 5 seconds");
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    });
                });

                while let Some(message) = rx.next().await {
                    let _ = output.send(message).await;
                }
            },
        )
    })
}

/// Called whenever a device (re)connects.
///
/// Two things happen here:
///  * the refresh daemon is woken so that apps which are due (or close to
///    expiring) are renewed straight away, and
///  * "sync over Wi-Fi" is enabled on the device while it is attached over USB,
///    which is what allows us to refresh apps later on while the phone is only
///    on the same network as this computer.
fn prepare_wireless_refresh(device: Device) {
    crate::refresh::wake_refresh_daemon();

    tokio::spawn(async move {
        if device.is_network() {
            return;
        }

        // Pairing (if needed) and enabling wireless sync. iOS only asks the
        // user to trust the computer once; after that everything happens
        // automatically.
        match device.prepare_wireless().await {
            Ok(()) => {
                log::info!(
                    "Wi-Fi renewals enabled for {} ({})",
                    device.name,
                    device.udid
                );
            }
            Err(e) => {
                log::warn!(
                    "Could not enable Wi-Fi renewals for {} yet: {e}",
                    device.udid
                );
                return;
            }
        }

        // Remember the phone's Wi-Fi hardware address: renewals can then find
        // it through the ARP table on networks that filter mDNS.
        if let Some(mac) = device.wifi_mac().await {
            log::info!("{} Wi-Fi address is {mac}", device.udid);

            let directory = crate::defaults::get_data_path().join("wifi_macs");
            let _ = std::fs::create_dir_all(&directory);
            let _ = std::fs::write(directory.join(format!("{}.txt", device.udid)), &mac);
        }

        // Verify the wireless path straight away, while the cable is still
        // connected: the phone starts advertising itself over mDNS as soon as
        // wireless sync is on. This turns "does it work after unplugging?" into
        // an immediate, visible answer in the log.
        for attempt in 1..=4 {
            tokio::time::sleep(Duration::from_secs(5)).await;

            match device.verify_wireless().await {
                Ok(address) => {
                    log::info!(
                        "Wi-Fi connection verified at {address} - renewals will work without the cable"
                    );
                    return;
                }
                Err(err) => {
                    log::warn!("Wi-Fi check {attempt}/4 for {} failed: {err}", device.udid);
                }
            }
        }

        log::warn!(
            "{} is paired and wireless sync is on, but it could not be reached over Wi-Fi yet; \
             make sure the phone is unlocked and on the same network",
            device.udid
        );
    });
}

pub(crate) fn tray_subscription() -> Subscription<Message> {
    Subscription::run(|| {
        iced::stream::channel(
            100,
            |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                use iced::futures::{SinkExt, StreamExt};
                let (tx, mut rx) = iced::futures::channel::mpsc::unbounded::<Message>();

                std::thread::spawn(move || {
                    let menu_channel = MenuEvent::receiver();
                    let tray_channel = TrayIconEvent::receiver();
                    loop {
                        if let Ok(event) = menu_channel.try_recv() {
                            let _ = tx.unbounded_send(Message::TrayMenuClicked(event.id));
                        }

                        if let Ok(event) = tray_channel.try_recv() {
                            match event {
                                TrayIconEvent::DoubleClick {
                                    button: tray_icon::MouseButton::Left,
                                    ..
                                } => {
                                    let _ = tx.unbounded_send(Message::TrayIconClicked);
                                }
                                _ => {}
                            }
                        }

                        #[cfg(target_os = "linux")]
                        {
                            let _ = tx.unbounded_send(Message::GtkTick);
                        }

                        #[cfg(target_os = "macos")]
                        {
                            let _ = tx.unbounded_send(Message::MacOsActivationTick);
                        }

                        std::thread::sleep(std::time::Duration::from_millis(32));
                    }
                });

                while let Some(message) = rx.next().await {
                    let _ = output.send(message).await;
                }
            },
        )
    })
}

pub(crate) fn tray_menu_refresh_subscription() -> Subscription<Message> {
    Subscription::run(|| {
        iced::stream::channel(
            10,
            |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                use iced::futures::{SinkExt, StreamExt};
                let (tx, mut rx) = iced::futures::channel::mpsc::unbounded::<Message>();

                std::thread::spawn(move || {
                    loop {
                        std::thread::sleep(std::time::Duration::from_secs(30));
                        let _ = tx.unbounded_send(Message::UpdateTrayMenu);
                    }
                });

                while let Some(message) = rx.next().await {
                    let _ = output.send(message).await;
                }
            },
        )
    })
}

pub(crate) fn certificate_reset_subscription() -> Subscription<Message> {
    Subscription::run(|| {
        iced::stream::channel(
            10,
            |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                use iced::futures::{SinkExt, StreamExt};
                let (tx, mut rx) = iced::futures::channel::mpsc::unbounded::<Message>();

                std::thread::spawn(move || {
                    while let Some(request) = crate::certificate_reset::wait_for_request() {
                        let _ = tx.unbounded_send(Message::CertificateResetRequested(request));
                    }
                });

                while let Some(message) = rx.next().await {
                    let _ = output.send(message).await;
                }
            },
        )
    })
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
pub(crate) fn relaunch_subscription() -> Subscription<Message> {
    Subscription::run(|| {
        iced::stream::channel(
            10,
            |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                use iced::futures::{SinkExt, StreamExt};
                let (tx, mut rx) = iced::futures::channel::mpsc::unbounded::<Message>();

                if let Err(err) = crate::relaunch::start_listener({
                    let tx = tx.clone();
                    move || {
                        let _ = tx.unbounded_send(Message::RelaunchRequested);
                    }
                }) {
                    log::warn!("Failed to start relaunch listener: {err}");
                }

                while let Some(message) = rx.next().await {
                    let _ = output.send(message).await;
                }
            },
        )
    })
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
pub(crate) fn relaunch_subscription() -> Subscription<Message> {
    Subscription::none()
}

pub(crate) fn file_hover_subscription() -> Subscription<Message> {
    let window_events = window::events().filter_map(|(_id, event)| match event {
        window::Event::FileHovered(_) => Some(Message::MainScreen(general::Message::FilesHovered)),
        window::Event::FilesHoveredLeft => {
            Some(Message::MainScreen(general::Message::FilesHoveredLeft))
        }
        window::Event::FileDropped(path) => {
            Some(Message::MainScreen(general::Message::FilesDropped(vec![
                path,
            ])))
        }
        _ => None,
    });

    window_events
}

pub(crate) fn installation_progress_listener(
    progress_rx: Option<Arc<std::sync::Mutex<std::sync::mpsc::Receiver<(String, i32)>>>>,
) -> Subscription<(String, i32)> {
    match progress_rx {
        Some(rx) => {
            struct State {
                rx: Arc<std::sync::Mutex<std::sync::mpsc::Receiver<(String, i32)>>>,
            }

            impl std::hash::Hash for State {
                fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                    Arc::as_ptr(&self.rx).hash(state);
                }
            }

            let state = State { rx };
            Subscription::run_with(state, |state| {
                let rx = state.rx.clone();
                iced::stream::channel(
                    100,
                    move |mut output: iced::futures::channel::mpsc::Sender<(String, i32)>| async move {
                        use iced::futures::{SinkExt, StreamExt};

                        let (tx, mut rx_stream) =
                            iced::futures::channel::mpsc::unbounded::<(String, i32)>();

                        let rx_thread = rx.clone();
                        std::thread::spawn(move || {
                            loop {
                                let message = {
                                    if let Ok(guard) = rx_thread.lock() {
                                        guard.try_recv().ok()
                                    } else {
                                        None
                                    }
                                };

                                if let Some((status, progress)) = message {
                                    let _ = tx.unbounded_send((status, progress));
                                }

                                std::thread::sleep(std::time::Duration::from_millis(100));
                            }
                        });

                        while let Some(message) = rx_stream.next().await {
                            let _ = output.send(message).await;
                        }
                    },
                )
            })
        }
        None => Subscription::none(),
    }
}

pub(crate) async fn run_installation(
    package: &plume_utils::Package,
    device: Option<&Device>,
    options: &plume_utils::SignerOptions,
    account: Option<&plume_store::GsaAccount>,
    mut store: Option<&mut plume_store::AccountStore>,
    tx: &std::sync::mpsc::Sender<(String, i32)>,
) -> Result<(), String> {
    use plume_core::{AnisetteConfiguration, CertificateIdentity, developer::DeveloperSession};
    use plume_utils::{Signer, SignerInstallMode, SignerMode};

    let package_file: Bundle;
    let mut options = options.clone();
    let send = |msg: String, progress: i32| {
        let _ = tx.send((msg, progress));
    };

    let source_bundle_id = package.get_bundle_identifier();
    let is_update = match (store.as_deref(), device, source_bundle_id.as_deref()) {
        (Some(store), Some(device), Some(bundle_id)) => {
            store.has_refresh_app_for_source(&device.udid, bundle_id)
        }
        _ => false,
    };
    if is_update {
        send(
            t!(
                "progress_updating_application",
                version = package.get_version().unwrap_or_else(|| "?".to_string())
            )
            .to_string(),
            10,
        );
    } else {
        send("Preparing package...".to_string(), 10);
    }

    match options.mode {
        SignerMode::Pem => {
            let Some(account) = account else {
                return Err("GSA account is required for PEM signing".to_string());
            };

            send("Ensuring account is valid...".to_string(), 20);

            let session = DeveloperSession::new(
                account.adsid().clone(),
                account.xcode_gs_token().clone(),
                AnisetteConfiguration::default()
                    .set_configuration_path(crate::defaults::get_data_path()),
            )
            .await
            .map_err(|e| e.to_string())?;

            let teams_response = session.qh_list_teams().await.map_err(|e| e.to_string())?;

            if teams_response.teams.is_empty() {
                return Err("No teams available for this account".to_string());
            }

            let team_id = account.team_id();

            if !team_id.is_empty() && !teams_response.teams.iter().any(|t| &t.team_id == team_id) {
                return Err(format!(
                    "Stored team ID '{}' not found in available teams. Please update your team selection in Settings.",
                    team_id
                ));
            }

            let team_id = if team_id.is_empty() {
                &teams_response.teams[0].team_id
            } else {
                team_id
            };

            let mut on_certificate_reset = || {
                send(crate::certificate_reset::WARNING.to_string(), 20);
                crate::certificate_reset::confirm()
            };
            let identity = CertificateIdentity::new_with_session(
                &session,
                crate::defaults::get_data_path(),
                None,
                team_id,
                false,
                Some(&mut on_certificate_reset),
            )
            .await
            .map_err(|e| e.to_string())?;

            send("Ensuring device is registered...".to_string(), 30);

            if let Some(dev) = &device {
                session
                    .qh_ensure_device(team_id, &dev.name, &dev.udid)
                    .await
                    .map_err(|e| e.to_string())?;
            }

            send("Extracting package...".to_string(), 50);

            let mut signer = Signer::new(Some(identity), options.clone());

            let bundle = package.get_package_bundle().map_err(|e| e.to_string())?;

            send("Signing package...".to_string(), 70);

            signer
                .modify_bundle(&bundle, &Some(team_id.clone()))
                .await
                .map_err(|e| e.to_string())?;
            signer
                .register_bundle(&bundle, &session, team_id, false)
                .await
                .map_err(|e| e.to_string())?;
            signer
                .sign_bundle(&bundle)
                .await
                .map_err(|e| e.to_string())?;

            options = signer.options.clone();
            package_file = bundle;
        }
        SignerMode::Adhoc => {
            send("Extracting package...".to_string(), 50);

            let mut signer = Signer::new(None, options.clone());

            let bundle = package.get_package_bundle().map_err(|e| e.to_string())?;

            send("Signing package...".to_string(), 70);

            signer
                .modify_bundle(&bundle, &None)
                .await
                .map_err(|e| e.to_string())?;
            signer
                .sign_bundle(&bundle)
                .await
                .map_err(|e| e.to_string())?;

            options = signer.options.clone();
            package_file = bundle;
        }
        _ => {
            send("Extracting package...".to_string(), 50);

            let bundle = package.get_package_bundle().map_err(|e| e.to_string())?;

            package_file = bundle;
        }
    }

    match options.install_mode {
        SignerInstallMode::Install => {
            if let Some(dev) = &device {
                if !dev.is_mac {
                    send("Sending to device...".to_string(), 70);

                    let tx_clone = tx.clone();
                    dev.install_app(&package_file.bundle_dir(), move |progress: i32| {
                        let tx = tx_clone.clone();
                        // Some libraries expect this future to be processed.
                        // We ensure it sends and resolves immediately.
                        Box::pin(async move {
                            let _ = tx.send(("Installing...".to_string(), 70 + (progress / 5)));
                        })
                    })
                    .await
                    .map_err(|e| format!("Install error: {}", e))?;

                    if options.app.supports_pairing_file() {
                        if let (Some(custom_identifier), Some(pairing_file_bundle_path)) = (
                            options.custom_identifier.as_ref(),
                            options.app.pairing_file_path(),
                        ) {
                            let _ = dev
                                .install_pairing_record(
                                    custom_identifier,
                                    &pairing_file_bundle_path,
                                )
                                .await;
                        }
                    }
                } else {
                    send("Installing...".to_string(), 90);

                    plume_utils::install_app_mac(&package_file.bundle_dir())
                        .await
                        .map_err(|e| e.to_string())?;
                }
            } else {
                return Err("No device connected for installation".to_string());
            }
        }
        SignerInstallMode::Export => {
            send("Exporting...".to_string(), 90);

            let archive_path = package
                .get_archive_based_on_path(&package_file.bundle_dir())
                .map_err(|e| e.to_string())?;

            let file = rfd::AsyncFileDialog::new()
                .set_title("Save Package As")
                .set_file_name(
                    archive_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("package.ipa"),
                )
                .save_file()
                .await;

            if let Some(save_path) = file {
                tokio::fs::copy(&archive_path, &save_path.path())
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }
    }

    if options.refresh && options.mode == SignerMode::Pem {
        send("Saving for refresh...".to_string(), 75);
        let path = get_data_path().join("refresh_store");
        tokio::fs::create_dir_all(&path)
            .await
            .map_err(|e| e.to_string())?;

        let original_name = package_file
            .bundle_dir()
            .file_name()
            .unwrap()
            .to_string_lossy();
        let uuid = uuid::Uuid::new_v4();
        let dest_name = if let Some(dot_pos) = original_name.rfind('.') {
            let (name, ext) = original_name.split_at(dot_pos);
            format!("{}-{}{}", name, uuid, ext)
        } else {
            format!("{}-{}", original_name, uuid)
        };
        let dest_path = path.join(dest_name);

        plume_utils::copy_dir_recursively(&package_file.bundle_dir(), &dest_path)
            .await
            .map_err(|e| e.to_string())?;

        if let (Some(dev), Some(account), Some(store)) = (&device, &account, store.as_mut()) {
            let embedded_prov_path = dest_path.join("embedded.mobileprovision");

            let provision_path = if embedded_prov_path.exists() {
                Some(embedded_prov_path)
            } else {
                None
            };

            if let Some(prov_path) = provision_path {
                use plume_core::MobileProvision;

                if let Ok(provision) = MobileProvision::load_with_path(&prov_path) {
                    let expiration_date = provision.expiration_date().clone();
                    let scheduled_refresh = expiration_date
                        .to_xml_format()
                        .parse::<chrono::DateTime<chrono::Utc>>()
                        .unwrap_or_else(|_| chrono::Utc::now() + chrono::Duration::days(4));
                    let scheduled_refresh = scheduled_refresh
                        - chrono::Duration::days(store.refresh_days_before() as i64);

                    let mut refresh_device = store
                        .get_refresh_device(&dev.udid)
                        .cloned()
                        .unwrap_or_else(|| plume_store::RefreshDevice {
                            udid: dev.udid.clone(),
                            name: dev.name.clone(),
                            account: account.email().clone(),
                            apps: Vec::new(),
                            is_mac: dev.is_mac,
                        });
                    let existing_index = refresh_device
                        .apps
                        .iter()
                        .position(|app| app.bundle_id == package_file.get_bundle_identifier());
                    let existing_auto_refresh = existing_index
                        .map(|index| refresh_device.apps[index].auto_refresh)
                        .unwrap_or(true);
                    let old_source_path =
                        existing_index.map(|index| refresh_device.apps[index].path.clone());

                    let refresh_app = plume_store::RefreshApp {
                        name: package_file.get_name(),
                        bundle_id: package_file.get_bundle_identifier(),
                        path: dest_path.clone(),
                        // Remember which Apple ID this app was signed with, so
                        // renewals keep using the right one even when several
                        // accounts are used on the same phone.
                        account: Some(account.email().clone()),
                        auto_refresh: existing_auto_refresh,
                        scheduled_refresh,
                    };

                    if let Some(existing_index) = existing_index {
                        refresh_device.apps[existing_index] = refresh_app;
                    } else {
                        refresh_device.apps.push(refresh_app);
                    }

                    store
                        .add_or_update_refresh_device_sync(refresh_device)
                        .map_err(|e| e.to_string())?;

                    if let Some(old_source_path) = old_source_path {
                        if old_source_path != dest_path && old_source_path.starts_with(&path) {
                            if let Err(error) = tokio::fs::remove_dir_all(&old_source_path).await {
                                log::warn!(
                                    "Updated app but could not remove old refresh source {:?}: {}",
                                    old_source_path,
                                    error
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    send("Finished!".to_string(), 100);

    Ok(())
}

#[allow(dead_code)]
pub(crate) async fn export_certificate(account: plume_store::GsaAccount) -> Result<(), String> {
    use plume_core::{AnisetteConfiguration, CertificateIdentity, developer::DeveloperSession};

    let session = DeveloperSession::new(
        account.adsid().clone(),
        account.xcode_gs_token().clone(),
        AnisetteConfiguration::default().set_configuration_path(crate::defaults::get_data_path()),
    )
    .await
    .map_err(|e| e.to_string())?;

    let teams_response = session.qh_list_teams().await.map_err(|e| e.to_string())?;

    if teams_response.teams.is_empty() {
        return Err("No teams available for this account".to_string());
    }

    let team_id = account.team_id();

    if !team_id.is_empty() && !teams_response.teams.iter().any(|t| &t.team_id == team_id) {
        return Err(format!(
            "Stored team ID '{}' not found in available teams. Please update your team selection in Settings.",
            team_id
        ));
    }

    let team_id = if team_id.is_empty() {
        &teams_response.teams[0].team_id
    } else {
        team_id
    };

    let mut on_certificate_reset = crate::certificate_reset::confirm;
    let identity = CertificateIdentity::new_with_session(
        &session,
        crate::defaults::get_data_path(),
        None,
        team_id,
        true,
        Some(&mut on_certificate_reset),
    )
    .await
    .map_err(|e| e.to_string())?;

    let Some(p12_data) = identity.p12_data else {
        return Err("Missing p12 data".to_string());
    };

    let archive_path =
        crate::defaults::get_data_path().join(format!("{}_certificate.p12", team_id));
    tokio::fs::write(&archive_path, p12_data)
        .await
        .map_err(|e| e.to_string())?;

    let file = rfd::AsyncFileDialog::new()
        .set_title("Save Certificate As")
        .set_file_name(
            archive_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("certificate.p12"),
        )
        .save_file()
        .await;

    if let Some(save_path) = file {
        tokio::fs::copy(&archive_path, &save_path.path())
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

pub(crate) async fn fetch_teams(
    account: &plume_store::GsaAccount,
) -> Result<Vec<crate::screen::settings::Team>, String> {
    use plume_core::{AnisetteConfiguration, developer::DeveloperSession};

    let session = DeveloperSession::new(
        account.adsid().clone(),
        account.xcode_gs_token().clone(),
        AnisetteConfiguration::default().set_configuration_path(crate::defaults::get_data_path()),
    )
    .await
    .map_err(|e| e.to_string())?;

    let teams_response = session.qh_list_teams().await.map_err(|e| e.to_string())?;

    Ok(teams_response
        .teams
        .into_iter()
        .map(|t| crate::screen::settings::Team {
            name: t.name,
            id: t.team_id,
        })
        .collect())
}
