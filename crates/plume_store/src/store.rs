use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use plume_core::Error;

use crate::{GsaAccount, RefreshDevice};

/// How many days before a provisioning profile expires we should renew it.
/// Free Apple IDs only allow 7 day profiles, so the default leaves a couple of
/// days of slack in case the computer is offline for a while.
pub const DEFAULT_REFRESH_DAYS_BEFORE: u32 = 3;

fn default_refresh_days_before() -> u32 {
    DEFAULT_REFRESH_DAYS_BEFORE
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccountStore {
    selected_account: Option<String>,      // Email
    accounts: HashMap<String, GsaAccount>, // Email -> GsaAccount
    #[serde(default)]
    refreshes: HashMap<String, RefreshDevice>, // UDID -> RefreshDevice (apps?)
    #[serde(default)]
    locale: Option<String>, // None = system locale
    #[serde(default = "default_refresh_days_before")]
    refresh_days_before: u32, // Renew this many days before expiry
    /// Whether the user wants the "remote pairing" flow when installing pairing
    /// files. Kept here so the choice survives leaving the tools screen and
    /// restarting the application.
    #[serde(default)]
    remote_pairing: bool,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl Default for AccountStore {
    fn default() -> Self {
        Self {
            selected_account: None,
            accounts: HashMap::new(),
            refreshes: HashMap::new(),
            locale: None,
            refresh_days_before: DEFAULT_REFRESH_DAYS_BEFORE,
            remote_pairing: false,
            path: None,
        }
    }
}

impl AccountStore {
    pub async fn load(path: &Option<PathBuf>) -> Result<Self, Error> {
        if let Some(path) = path {
            let mut settings = if !path.exists() {
                Self::default()
            } else {
                let contents = tokio::fs::read_to_string(path).await?;
                serde_json::from_str(&contents)?
            };
            settings.path = Some(path.clone());
            Ok(settings)
        } else {
            Ok(Self::default())
        }
    }

    pub fn load_sync(path: &Option<PathBuf>) -> Result<Self, Error> {
        if let Some(path) = path {
            let mut settings = if !path.exists() {
                Self::default()
            } else {
                let contents = std::fs::read_to_string(path)?;
                serde_json::from_str(&contents)?
            };
            settings.path = Some(path.clone());
            Ok(settings)
        } else {
            Ok(Self::default())
        }
    }

    pub async fn save(&self) -> Result<(), Error> {
        if let Some(path) = &self.path {
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            tokio::fs::write(path, serde_json::to_string_pretty(self)?).await?;
        }
        Ok(())
    }

    pub fn save_sync(&self) -> Result<(), Error> {
        if let Some(path) = &self.path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        }
        Ok(())
    }

    pub fn accounts(&self) -> &HashMap<String, GsaAccount> {
        &self.accounts
    }

    pub fn path(&self) -> Option<PathBuf> {
        self.path.clone()
    }

    pub fn get_account(&self, email: &str) -> Option<&GsaAccount> {
        self.accounts.get(email)
    }

    pub async fn accounts_add(&mut self, account: GsaAccount) -> Result<(), Error> {
        let email = account.email().clone();
        self.accounts.insert(email.clone(), account);
        self.selected_account = Some(email);
        self.save().await
    }

    pub fn accounts_add_sync(&mut self, account: GsaAccount) -> Result<(), Error> {
        let email = account.email().clone();
        self.accounts.insert(email.clone(), account);
        self.selected_account = Some(email);
        self.save_sync()
    }

    pub async fn accounts_remove(&mut self, email: &str) -> Result<(), Error> {
        self.accounts.remove(email);
        if self.selected_account.as_ref() == Some(&email.to_string()) {
            self.selected_account = None;
        }
        self.save().await
    }

    pub fn accounts_remove_sync(&mut self, email: &str) -> Result<(), Error> {
        self.accounts.remove(email);
        if self.selected_account.as_ref() == Some(&email.to_string()) {
            self.selected_account = None;
        }
        self.save_sync()
    }

    pub async fn account_select(&mut self, email: &str) -> Result<(), Error> {
        if self.accounts.contains_key(email) {
            self.selected_account = Some(email.to_string());
            self.save().await
        } else {
            Err(Error::Parse) // we need better errors
        }
    }

    pub fn account_select_sync(&mut self, email: &str) -> Result<(), Error> {
        if self.accounts.contains_key(email) {
            self.selected_account = Some(email.to_string());
            self.save_sync()
        } else {
            Err(Error::Parse) // we need better errors
        }
    }

    pub fn selected_account(&self) -> Option<&GsaAccount> {
        if let Some(email) = &self.selected_account {
            self.accounts.get(email)
        } else {
            None
        }
    }

    /// How many apps are currently registered to be renewed with `email`.
    ///
    /// A free Apple ID may only sign three apps at a time, so this is what the
    /// automatic account picker balances.
    pub fn app_count_for_account(&self, email: &str) -> usize {
        self.refreshes
            .values()
            .flat_map(|device| {
                device
                    .apps
                    .iter()
                    .map(move |app| (device.account.as_str(), app))
            })
            .filter(|(device_account, app)| {
                // Apps remember their own account; older entries fall back to
                // the device's account.
                app.account
                    .as_deref()
                    .unwrap_or(device_account)
                    .eq_ignore_ascii_case(email)
            })
            .count()
    }

    /// Picks the Apple ID to sign a new app with.
    ///
    /// * an app that is already being renewed keeps the account it was signed
    ///   with before, and
    /// * otherwise the account with the fewest apps wins (the selected one is
    ///   preferred when the counts are equal), which spreads apps evenly across
    ///   the three-app limit of each free Apple ID.
    pub fn account_for_new_app(&self, bundle_id: Option<&str>) -> Option<GsaAccount> {
        if let Some(bundle_id) = bundle_id {
            for device in self.refreshes.values() {
                if let Some(app) = device
                    .apps
                    .iter()
                    .find(|app| app.bundle_id.as_deref() == Some(bundle_id))
                {
                    let email = app.account.clone().unwrap_or_else(|| device.account.clone());
                    if let Some(account) = self.get_account(&email) {
                        log::info!("{bundle_id} was signed with {email} before; reusing that account");
                        return Some(account.clone());
                    }
                }
            }
        }

        let preferred = self.selected_account().map(|account| account.email().clone());

        let mut accounts: Vec<_> = self.accounts.values().cloned().collect();
        accounts.sort_by(|left, right| {
            let left_count = self.app_count_for_account(left.email());
            let right_count = self.app_count_for_account(right.email());

            left_count
                .cmp(&right_count)
                .then_with(|| {
                    // Prefer the account the user picked when both are equally
                    // loaded.
                    let left_preferred = Some(left.email()) == preferred.as_ref();
                    let right_preferred = Some(right.email()) == preferred.as_ref();
                    right_preferred.cmp(&left_preferred)
                })
                .then_with(|| left.email().cmp(right.email()))
        });

        let account = accounts.into_iter().next()?;
        let count = self.app_count_for_account(account.email());
        log::info!(
            "Auto-assigned Apple ID {} (currently {count} app(s)) for the new app",
            account.email()
        );

        Some(account)
    }

    pub fn locale(&self) -> Option<&str> {
        self.locale.as_deref()
    }

    pub fn set_locale_sync(&mut self, locale: Option<String>) -> Result<(), Error> {
        self.locale = locale;
        self.save_sync()
    }

    /// Number of days before the profile expires that a renewal should happen.
    pub fn refresh_days_before(&self) -> u32 {
        if self.refresh_days_before == 0 {
            DEFAULT_REFRESH_DAYS_BEFORE
        } else {
            self.refresh_days_before.clamp(1, 6)
        }
    }

    pub fn set_refresh_days_before_sync(&mut self, days: u32) -> Result<(), Error> {
        self.refresh_days_before = days.clamp(1, 6);
        self.save_sync()
    }

    pub fn remote_pairing(&self) -> bool {
        self.remote_pairing
    }

    pub fn set_remote_pairing_sync(&mut self, enabled: bool) -> Result<(), Error> {
        self.remote_pairing = enabled;
        self.save_sync()
    }

    pub async fn set_refresh_days_before(&mut self, days: u32) -> Result<(), Error> {
        self.refresh_days_before = days.clamp(1, 6);
        self.save().await
    }

    pub async fn accounts_add_from_session(
        &mut self,
        email: String,
        account: plume_core::auth::Account,
    ) -> Result<(), Error> {
        let first_name = account.get_name().0;
        let s = plume_core::developer::DeveloperSession::using_account(account).await?;
        let teams_response = s.qh_list_teams().await?;
        let adsid = s.adsid().clone();
        let xcode_gs_token = s.xcode_gs_token().clone();

        let team_id = if teams_response.teams.is_empty() {
            "".to_string()
        } else {
            teams_response.teams[0].team_id.clone()
        };

        let account = GsaAccount::new(email, first_name, adsid, xcode_gs_token, team_id);

        self.accounts_add(account).await?;

        Ok(())
    }

    pub async fn update_account_team(&mut self, email: &str, team_id: String) -> Result<(), Error> {
        if let Some(account) = self.accounts.get_mut(email) {
            account.set_team_id(team_id);
            self.save().await
        } else {
            Err(Error::Parse)
        }
    }

    pub fn update_account_team_sync(&mut self, email: &str, team_id: String) -> Result<(), Error> {
        if let Some(account) = self.accounts.get_mut(email) {
            account.set_team_id(team_id);
            self.save_sync()
        } else {
            Err(Error::Parse)
        }
    }

    pub fn refreshes(&self) -> &HashMap<String, RefreshDevice> {
        &self.refreshes
    }

    pub fn get_refresh_device(&self, udid: &str) -> Option<&RefreshDevice> {
        self.refreshes.get(udid)
    }

    pub async fn add_or_update_refresh_device(
        &mut self,
        device: RefreshDevice,
    ) -> Result<(), Error> {
        self.refreshes.insert(device.udid.clone(), device);
        self.save().await
    }

    pub fn add_or_update_refresh_device_sync(
        &mut self,
        device: RefreshDevice,
    ) -> Result<(), Error> {
        self.refreshes.insert(device.udid.clone(), device);
        self.save_sync()
    }

    pub async fn remove_refresh_device(&mut self, udid: &str) -> Result<(), Error> {
        self.refreshes.remove(udid);
        self.save().await
    }

    pub fn remove_refresh_device_sync(&mut self, udid: &str) -> Result<(), Error> {
        self.refreshes.remove(udid);
        self.save_sync()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn refresh_days_defaults_to_the_constant() {
        let store = AccountStore::default();
        assert_eq!(store.refresh_days_before(), DEFAULT_REFRESH_DAYS_BEFORE);
    }

    #[test]
    fn refresh_days_is_sanitised() {
        let mut store = AccountStore::default();

        store.refresh_days_before = 0;
        assert_eq!(store.refresh_days_before(), DEFAULT_REFRESH_DAYS_BEFORE);

        store.refresh_days_before = 2;
        assert_eq!(store.refresh_days_before(), 2);

        store.refresh_days_before = 99;
        assert_eq!(store.refresh_days_before(), 6);
    }

    #[test]
    fn older_account_files_still_load() {
        // Files written before the auto-renew setting existed must keep working.
        let json = r#"{"selected_account":null,"accounts":{},"refreshes":{}}"#;
        let store: AccountStore = serde_json::from_str(json).unwrap();

        assert_eq!(store.refresh_days_before(), DEFAULT_REFRESH_DAYS_BEFORE);
        assert!(store.locale().is_none());
        assert!(store.accounts().is_empty());
    }

    #[test]
    fn refresh_days_survives_a_round_trip() {
        let mut store = AccountStore::default();
        store.refresh_days_before = 5;

        let json = serde_json::to_string(&store).unwrap();
        let restored: AccountStore = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.refresh_days_before(), 5);
    }

    fn test_account(email: &str) -> GsaAccount {
        GsaAccount::new(
            email.to_string(),
            "Test".to_string(),
            format!("adsid-{email}"),
            format!("token-{email}"),
            "TEAMID".to_string(),
        )
    }

    fn test_device(account: &str, apps: Vec<(&str, Option<&str>)>) -> RefreshDevice {
        RefreshDevice {
            udid: format!("udid-{account}"),
            name: "Test Phone".to_string(),
            account: account.to_string(),
            is_mac: false,
            apps: apps
                .into_iter()
                .map(|(bundle_id, app_account)| crate::RefreshApp {
                    path: PathBuf::from(format!("/tmp/{bundle_id}.app")),
                    name: Some(bundle_id.to_string()),
                    bundle_id: Some(bundle_id.to_string()),
                    account: app_account.map(str::to_string),
                    scheduled_refresh: Utc::now(),
                })
                .collect(),
        }
    }

    #[test]
    fn account_with_fewest_apps_is_picked() {
        let mut store = AccountStore::default();
        store.accounts.insert(
            "busy@example.com".to_string(),
            test_account("busy@example.com"),
        );
        store.accounts.insert(
            "free@example.com".to_string(),
            test_account("free@example.com"),
        );

        store.refreshes.insert(
            "udid-1".to_string(),
            test_device("busy@example.com", vec![("a", None), ("b", None)]),
        );

        let picked = store.account_for_new_app(Some("com.example.new")).unwrap();
        assert_eq!(picked.email(), "free@example.com");
    }

    #[test]
    fn an_app_sticks_to_the_account_that_signed_it() {
        let mut store = AccountStore::default();
        store.accounts.insert(
            "first@example.com".to_string(),
            test_account("first@example.com"),
        );
        store.accounts.insert(
            "second@example.com".to_string(),
            test_account("second@example.com"),
        );
        // "second" is idle, but the app was signed with "first" before.
        store.refreshes.insert(
            "udid-1".to_string(),
            test_device(
                "first@example.com",
                vec![("com.example.old", Some("first@example.com"))],
            ),
        );

        let picked = store
            .account_for_new_app(Some("com.example.old"))
            .unwrap();
        assert_eq!(picked.email(), "first@example.com");
    }
}
