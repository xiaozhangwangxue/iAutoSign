//! Discover iOS apps installed locally by AutoSign on Apple-silicon Macs.
//!
//! Older versions only displayed apps installed with the Auto Refresh option.
//! Discovered apps are therefore imported as manual-only entries: they become
//! visible and renewable without silently enabling background renewal.

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub fn sync_installed_apps(store: &mut plume_store::AccountStore) -> usize {
    use chrono::{DateTime, Duration, Utc};
    use plume_core::MobileProvision;
    use plume_store::{RefreshApp, RefreshDevice};
    use std::path::{Path, PathBuf};

    fn inner_bundle(outer: &Path) -> Option<PathBuf> {
        let wrapper = outer.join("Wrapper");
        let expected = wrapper.join(outer.file_name()?);
        if expected.join("Info.plist").is_file() {
            return Some(expected);
        }

        std::fs::read_dir(wrapper)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.extension().and_then(|value| value.to_str()) == Some("app")
                    && path.join("Info.plist").is_file()
            })
    }

    fn app_metadata(path: &Path) -> Option<(String, String)> {
        let plist = plist::Value::from_file(path.join("Info.plist")).ok()?;
        let values = plist.as_dictionary()?;
        let name = values
            .get("CFBundleDisplayName")
            .or_else(|| values.get("CFBundleName"))
            .and_then(plist::Value::as_string)?
            .to_string();
        let bundle_id = values
            .get("CFBundleIdentifier")
            .and_then(plist::Value::as_string)?
            .to_string();
        Some((name, bundle_id))
    }

    let Some(udid) = plume_gestalt::get_udid() else {
        log::warn!("Cannot discover locally installed signed apps: Mac UDID is unavailable");
        return 0;
    };
    let Ok(entries) = std::fs::read_dir("/Applications/iOS") else {
        return 0;
    };

    let mut refresh_device =
        store
            .get_refresh_device(&udid)
            .cloned()
            .unwrap_or_else(|| RefreshDevice {
                udid: udid.clone(),
                name: "This Mac".to_string(),
                account: String::new(),
                apps: Vec::new(),
                is_mac: true,
            });
    let mut discovered = 0;
    let mut newly_added = 0;

    for outer in entries.filter_map(Result::ok).map(|entry| entry.path()) {
        if outer.extension().and_then(|value| value.to_str()) != Some("app") {
            continue;
        }

        let Some(bundle_path) = inner_bundle(&outer) else {
            continue;
        };
        let Some((name, bundle_id)) = app_metadata(&bundle_path) else {
            continue;
        };
        let Ok(provision) =
            MobileProvision::load_with_path(bundle_path.join("embedded.mobileprovision"))
        else {
            continue;
        };

        let team_id = provision
            .entitlements()
            .get("com.apple.developer.team-identifier")
            .and_then(plist::Value::as_string);
        let account_email = team_id.and_then(|team_id| {
            store
                .accounts()
                .values()
                .find(|account| account.team_id() == team_id)
                .map(|account| account.email().clone())
        });
        let expiration = provision
            .expiration_date()
            .to_xml_format()
            .parse::<DateTime<Utc>>()
            .unwrap_or_else(|_| Utc::now() + Duration::days(7));
        let scheduled_refresh = expiration - Duration::days(store.refresh_days_before() as i64);

        if let Some(existing) = refresh_device
            .apps
            .iter_mut()
            .find(|app| app.bundle_id.as_deref() == Some(bundle_id.as_str()))
        {
            // Preserve a dedicated auto-refresh copy while it exists. A
            // manual-only record safely uses the currently installed bundle.
            if !existing.path.exists() || !existing.auto_refresh {
                existing.path = bundle_path.clone();
            }
            existing.name = Some(name);
            existing.bundle_id = Some(bundle_id);
            if account_email.is_some() {
                existing.account = account_email.clone();
            }
            if !existing.auto_refresh {
                existing.scheduled_refresh = scheduled_refresh;
            }
        } else {
            refresh_device.apps.push(RefreshApp {
                path: bundle_path,
                name: Some(name),
                bundle_id: Some(bundle_id),
                account: account_email.clone(),
                auto_refresh: false,
                scheduled_refresh,
            });
            newly_added += 1;
        }

        if refresh_device.account.is_empty() {
            if let Some(email) = account_email {
                refresh_device.account = email;
            }
        }
        discovered += 1;
    }

    if discovered > 0 {
        if let Err(err) = store.add_or_update_refresh_device_sync(refresh_device) {
            log::error!("Failed to save locally installed signed apps: {err}");
            return 0;
        }
        if newly_added > 0 {
            log::info!("Discovered {newly_added} locally installed signed app(s)");
        }
    }

    discovered
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub fn sync_installed_apps(_store: &mut plume_store::AccountStore) -> usize {
    0
}
