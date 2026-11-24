//! Discover development-signed apps on connected iPhones and iPads.
//!
//! iOS exposes installed-app metadata but not the complete app bundle. A
//! renewal therefore needs both the phone entry and a matching local bundle
//! previously kept by AutoSign.

use chrono::{DateTime, Duration, Utc};
use plume_core::MobileProvision;
use plume_store::{AccountStore, RefreshApp, RefreshDevice};
use plume_utils::{Device, InstalledUserApp};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub async fn sync_installed_apps(
    mut store: AccountStore,
    device: Device,
) -> Result<AccountStore, String> {
    if device.is_mac {
        return Ok(store);
    }

    let installed = device
        .installed_user_apps()
        .await
        .map_err(|error| format!("Failed to read installed apps from {}: {error}", device.name))?;
    merge_installed_apps(&mut store, &device, installed)?;
    Ok(store)
}

fn merge_installed_apps(
    store: &mut AccountStore,
    device: &Device,
    installed: Vec<InstalledUserApp>,
) -> Result<usize, String> {
    let sources = local_sources(store);
    let mut refresh_device = store
        .get_refresh_device(&device.udid)
        .cloned()
        .unwrap_or_else(|| RefreshDevice {
            udid: device.udid.clone(),
            name: device.name.clone(),
            account: String::new(),
            apps: Vec::new(),
            is_mac: false,
        });
    refresh_device.name = device.name.clone();
    refresh_device.is_mac = false;

    let mut discovered = 0;
    for installed_app in installed
        .into_iter()
        .filter(|app| app.development_signed)
    {
        let source = sources.get(&installed_app.bundle_id).cloned();
        let account = source
            .as_ref()
            .and_then(|app| app.account.clone())
            .or_else(|| account_for_team(store, installed_app.team_id.as_deref()));
        let path = source
            .as_ref()
            .map(|app| app.path.clone())
            .unwrap_or_else(|| missing_source_path(&installed_app.bundle_id));
        let scheduled_refresh = source
            .as_ref()
            .and_then(|app| schedule_from_profile(&app.path, store.refresh_days_before()))
            .or_else(|| source.as_ref().map(|app| app.scheduled_refresh))
            .unwrap_or_else(Utc::now);

        let candidate = RefreshApp {
            path,
            name: Some(installed_app.name),
            bundle_id: Some(installed_app.bundle_id.clone()),
            account: account.clone(),
            auto_refresh: false,
            scheduled_refresh,
        };

        if let Some(existing) = refresh_device
            .apps
            .iter_mut()
            .find(|app| app.bundle_id.as_deref() == Some(installed_app.bundle_id.as_str()))
        {
            let auto_refresh = existing.auto_refresh;
            *existing = candidate;
            existing.auto_refresh = auto_refresh;
        } else {
            refresh_device.apps.push(candidate);
        }

        if refresh_device.account.is_empty() {
            if let Some(account) = account {
                refresh_device.account = account;
            }
        }
        discovered += 1;
    }

    if discovered > 0 {
        store
            .add_or_update_refresh_device_sync(refresh_device)
            .map_err(|error| format!("Failed to save apps from {}: {error}", device.name))?;
        log::info!(
            "Discovered {discovered} development-signed app(s) on {}",
            device.name
        );
    }

    Ok(discovered)
}

fn local_sources(store: &AccountStore) -> HashMap<String, RefreshApp> {
    let mut sources = HashMap::new();
    for (_, device) in store.refreshes() {
        for app in &device.apps {
            let Some(bundle_id) = app.bundle_id.as_ref() else {
                continue;
            };
            if app.path.join("Info.plist").is_file() {
                sources
                    .entry(bundle_id.clone())
                    .or_insert_with(|| app.clone());
            }
        }
    }
    sources
}

fn account_for_team(store: &AccountStore, team_id: Option<&str>) -> Option<String> {
    let team_id = team_id?;
    store
        .accounts()
        .values()
        .find(|account| account.team_id() == team_id)
        .map(|account| account.email().clone())
}

fn schedule_from_profile(path: &Path, refresh_days_before: u32) -> Option<DateTime<Utc>> {
    let profile = MobileProvision::load_with_path(path.join("embedded.mobileprovision")).ok()?;
    let expiration = profile
        .expiration_date()
        .to_xml_format()
        .parse::<DateTime<Utc>>()
        .ok()?;
    Some(expiration - Duration::days(refresh_days_before as i64))
}

fn missing_source_path(bundle_id: &str) -> PathBuf {
    let safe_name: String = bundle_id
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    crate::defaults::get_data_path()
        .join("missing_sources")
        .join(format!("{safe_name}.app"))
}
