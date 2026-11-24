pub(crate) mod general;
mod package;
mod progress;
pub(crate) mod settings;
mod signed_apps;
mod utilties;
mod windows;

use std::collections::{HashMap, HashSet, VecDeque};

use iced::Length::Fill;
use iced::widget::{button, column, container, pick_list, row, stack, text};
use iced::window;
use iced::{Element, Subscription, Task};

use plume_store::AccountStore;
use plume_utils::{Device, PlistInfoTrait, SignerOptions};

use crate::subscriptions;
use crate::tray::ImpactorTray;
use crate::{appearance, defaults};
use windows::login_window;

static REFRESH_DAEMON_DEVICES: std::sync::OnceLock<crate::refresh::ConnectedDevices> =
    std::sync::OnceLock::new();

pub fn set_refresh_daemon_devices(devices: crate::refresh::ConnectedDevices) {
    let _ = REFRESH_DAEMON_DEVICES.set(devices);
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Message {
    // Navigation
    NavigateToScreen(ImpactorScreenType),
    NextScreen,
    PreviousScreen,

    // Device management
    ComboBoxSelected(String),
    DeviceConnected(Device),
    DeviceDisconnected(u32),
    DeviceSignedAppsScanned {
        udid: String,
        result: Result<AccountStore, String>,
    },

    // Tray
    TrayMenuClicked(tray_icon::menu::MenuId),
    TrayIconClicked,
    #[cfg(target_os = "linux")]
    GtkTick,
    #[cfg(target_os = "macos")]
    MacOsActivationTick,

    // Refresh operations
    RefreshAppNow {
        udid: String,
        app_path: String,
    },
    RefreshAppCompleted {
        udid: String,
        app_path: String,
        result: Result<(), String>,
    },
    ForgetApp {
        udid: String,
        app_path: String,
    },
    UpdateTrayMenu,

    // Window management
    RelaunchRequested,
    ShowWindow,
    HideWindow,
    Quit,

    // Login window
    LoginWindowMessage(window::Id, login_window::Message),

    // Screen-specific messages
    MainScreen(general::Message),
    UtilitiesScreen(utilties::Message),
    SignedAppsScreen(signed_apps::Message),
    SettingsScreen(settings::Message),
    InstallerScreen(package::Message),
    ProgressScreen(progress::Message),
    CertificateResetRequested(crate::certificate_reset::ConfirmationRequest),
    ConfirmCertificateReset,
    CancelCertificateReset,

    // Installation
    StartInstallation,
}

pub struct Impactor {
    current_screen: ImpactorScreen,
    previous_screen: Option<Box<ImpactorScreen>>,
    devices: Vec<Device>,
    selected_device: Option<Device>,
    tray: Option<ImpactorTray>,
    main_window: Option<window::Id>,
    account_store: Option<AccountStore>,
    login_windows: std::collections::HashMap<window::Id, login_window::LoginWindow>,
    pending_installation: bool,
    certificate_reset_queue: VecDeque<crate::certificate_reset::ConfirmationRequest>,
    selected_locale: Option<String>,
    refreshing_apps: HashSet<String>,
    refresh_results: HashMap<String, Result<(), String>>,
    signed_app_scans: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImpactorScreenType {
    Main,
    Utilities,
    SignedApps,
    Settings,
    Installer,
    Progress,
}

enum ImpactorScreen {
    Main(general::GeneralScreen),
    Utilities(utilties::UtilitiesScreen),
    SignedApps(signed_apps::SignedAppsScreen),
    Settings(settings::SettingsScreen),
    Installer(package::PackageScreen),
    Progress(progress::ProgressScreen),
}

impl Impactor {
    pub fn new() -> (Self, Task<Message>) {
        let mut tray = ImpactorTray::new();
        let mut store = Self::init_account_store_sync();
        crate::macos_signed_apps::sync_installed_apps(&mut store);
        tray.update_refresh_apps(&store);
        let selected_locale = store.locale().map(str::to_string);
        if let Some(code) = &selected_locale {
            rust_i18n::set_locale(code);
        }
        let start_in_tray = crate::startup::start_in_tray_from_args();
        let (main_window, open_task) = if start_in_tray {
            (None, Task::none())
        } else {
            let (id, open_task) = window::open(defaults::default_window_settings());
            (Some(id), open_task.discard())
        };
        crate::macos_app::set_main_window_visible(true);
        crate::macos_app::reset_activation_state();

        (
            Self {
                current_screen: ImpactorScreen::Main(general::GeneralScreen::new()),
                previous_screen: None,
                devices: Vec::new(),
                selected_device: None,
                tray: Some(tray),
                main_window,
                account_store: Some(store),
                login_windows: std::collections::HashMap::new(),
                pending_installation: false,
                certificate_reset_queue: VecDeque::new(),
                selected_locale,
                refreshing_apps: HashSet::new(),
                refresh_results: HashMap::new(),
                signed_app_scans: HashSet::new(),
            },
            open_task,
        )
    }

    fn init_account_store_sync() -> AccountStore {
        let path = defaults::get_data_path().join("accounts.json");
        AccountStore::load_sync(&Some(path)).unwrap_or_default()
    }

    /// Builds the tools screen, restoring the stored preferences (the remote
    /// pairing switch used to reset whenever the screen was rebuilt).
    fn build_utilities_screen(&self) -> utilties::UtilitiesScreen {
        let remote_pairing = self
            .account_store
            .as_ref()
            .map(|store| store.remote_pairing())
            .unwrap_or(false);

        utilties::UtilitiesScreen::new(self.selected_device.clone(), remote_pairing)
    }

    fn respond_to_next_certificate_reset(&mut self, accepted: bool) {
        if let Some(request) = self.certificate_reset_queue.pop_front() {
            request.respond(accepted);
        }
    }

    fn cancel_pending_certificate_resets(&mut self) {
        while let Some(request) = self.certificate_reset_queue.pop_front() {
            request.respond(false);
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ComboBoxSelected(value) => {
                self.selected_device = self
                    .devices
                    .iter()
                    .find(|d| d.to_string() == value)
                    .cloned();

                if let ImpactorScreen::Utilities(_) = self.current_screen {
                    let rppairing_enabled = match &self.current_screen {
                        ImpactorScreen::Utilities(screen) => screen.rppairing_enabled,
                        _ => false,
                    };
                    self.current_screen = ImpactorScreen::Utilities(self.build_utilities_screen());
                    return Task::done(Message::UtilitiesScreen(utilties::Message::RefreshApps(
                        rppairing_enabled,
                    )));
                }

                Task::none()
            }
            Message::DeviceConnected(device) => {
                let mut tasks = Vec::new();
                let known_device = self
                    .devices
                    .iter_mut()
                    .find(|known| known.udid == device.udid);
                let is_new_device = known_device.is_none();
                if let Some(known_device) = known_device {
                    *known_device = device.clone();
                } else {
                    self.devices.push(device.clone());

                    if self.selected_device.is_none() && device.device_id != u32::MAX {
                        self.selected_device = Some(device.clone());
                    }
                }

                if let Some(daemon_devices) = REFRESH_DAEMON_DEVICES.get() {
                    if let Ok(mut devices) = daemon_devices.lock() {
                        devices.insert(device.udid.clone(), device.clone());
                    }
                }

                let has_refresh_record = self
                    .account_store
                    .as_ref()
                    .and_then(|store| store.get_refresh_device(&device.udid))
                    .is_some();
                if !device.is_mac
                    && (is_new_device || !has_refresh_record)
                    && self.signed_app_scans.insert(device.udid.clone())
                {
                    if let Some(store) = self.account_store.clone() {
                        let udid = device.udid.clone();
                        tasks.push(Task::perform(
                            run_signed_app_scan(store, device.clone()),
                            move |result| Message::DeviceSignedAppsScanned { udid, result },
                        ));
                    }
                }

                if let ImpactorScreen::Utilities(_) = self.current_screen {
                    let rppairing_enabled = match &self.current_screen {
                        ImpactorScreen::Utilities(screen) => screen.rppairing_enabled,
                        _ => false,
                    };
                    self.current_screen = ImpactorScreen::Utilities(self.build_utilities_screen());
                    tasks.push(Task::done(Message::UtilitiesScreen(
                        utilties::Message::RefreshApps(rppairing_enabled),
                    )));
                }

                Task::batch(tasks)
            }
            Message::DeviceSignedAppsScanned { udid, result } => {
                self.signed_app_scans.remove(&udid);
                match result {
                    Ok(store) => {
                        self.account_store = Some(store);
                        if let (Some(store), Some(tray)) = (&self.account_store, &mut self.tray) {
                            tray.update_refresh_apps(store);
                        }
                    }
                    Err(error) => {
                        log::warn!("Could not scan signed apps on {udid}: {error}");
                    }
                }
                Task::none()
            }
            Message::DeviceDisconnected(id) => {
                let udid = self
                    .devices
                    .iter()
                    .find(|d| d.device_id == id)
                    .map(|d| d.udid.clone());

                self.devices.retain(|d| d.device_id != id);

                if self.selected_device.as_ref().map(|d| d.device_id) == Some(id) {
                    self.selected_device = self.devices.first().cloned();
                }

                if let (Some(udid), Some(daemon_devices)) = (udid, REFRESH_DAEMON_DEVICES.get()) {
                    if let Ok(mut devices) = daemon_devices.lock() {
                        devices.remove(&udid);
                    }
                }

                if let ImpactorScreen::Utilities(_) = self.current_screen {
                    let rppairing_enabled = match &self.current_screen {
                        ImpactorScreen::Utilities(screen) => screen.rppairing_enabled,
                        _ => false,
                    };
                    self.current_screen = ImpactorScreen::Utilities(self.build_utilities_screen());
                    return Task::done(Message::UtilitiesScreen(utilties::Message::RefreshApps(
                        rppairing_enabled,
                    )));
                }

                Task::none()
            }
            Message::NavigateToScreen(screen_type) => {
                if screen_type == ImpactorScreenType::Settings {
                    if !matches!(self.current_screen, ImpactorScreen::Progress(_)) {
                        self.previous_screen = Some(Box::new(std::mem::replace(
                            &mut self.current_screen,
                            ImpactorScreen::Main(general::GeneralScreen::new()),
                        )));
                    }
                }

                self.navigate_to_screen(screen_type.clone());

                if screen_type == ImpactorScreenType::Utilities {
                    let rppairing_enabled = match &self.current_screen {
                        ImpactorScreen::Utilities(screen) => screen.rppairing_enabled,
                        _ => false,
                    };
                    return Task::done(Message::UtilitiesScreen(utilties::Message::RefreshApps(
                        rppairing_enabled,
                    )));
                }

                Task::none()
            }
            Message::NextScreen => {
                let next_screen = match self.current_screen {
                    ImpactorScreen::Main(_) => ImpactorScreenType::Installer,
                    ImpactorScreen::Utilities(_) => return Task::none(),
                    ImpactorScreen::SignedApps(_) => return Task::none(),
                    ImpactorScreen::Installer(_) => ImpactorScreenType::Progress,
                    ImpactorScreen::Settings(_) => return Task::none(),
                    ImpactorScreen::Progress(_) => return Task::none(),
                };

                self.navigate_to_screen(next_screen);
                Task::none()
            }
            Message::PreviousScreen => match &self.current_screen {
                ImpactorScreen::Main(_) => Task::none(),
                ImpactorScreen::Utilities(_) => {
                    self.navigate_to_screen(ImpactorScreenType::Main);
                    Task::none()
                }
                ImpactorScreen::SignedApps(_) => {
                    self.navigate_to_screen(ImpactorScreenType::Main);
                    Task::none()
                }
                ImpactorScreen::Installer(_) => {
                    self.navigate_to_screen(ImpactorScreenType::Main);
                    Task::none()
                }
                ImpactorScreen::Progress(_) => {
                    self.navigate_to_screen(ImpactorScreenType::Main);
                    Task::none()
                }
                ImpactorScreen::Settings(_) => {
                    if let Some(prev_screen) = self.previous_screen.take() {
                        self.current_screen = *prev_screen;
                    } else {
                        self.navigate_to_screen(ImpactorScreenType::Main);
                    }
                    Task::none()
                }
            },
            Message::TrayIconClicked => Task::done(Message::ShowWindow),
            Message::TrayMenuClicked(id) => {
                if let Some(tray) = &self.tray {
                    if let Some(action) = tray.get_action(&id) {
                        match action {
                            crate::tray::TrayAction::Show => Task::done(Message::ShowWindow),
                            crate::tray::TrayAction::Quit => Task::done(Message::Quit),
                            crate::tray::TrayAction::RefreshApp { udid, app_path } => {
                                Task::done(Message::RefreshAppNow {
                                    udid: udid.clone(),
                                    app_path: app_path.clone(),
                                })
                            }
                            crate::tray::TrayAction::ForgetApp { udid, app_path } => {
                                Task::done(Message::ForgetApp {
                                    udid: udid.clone(),
                                    app_path: app_path.clone(),
                                })
                            }
                        }
                    } else {
                        Task::none()
                    }
                } else {
                    Task::none()
                }
            }
            #[cfg(target_os = "linux")]
            Message::GtkTick => {
                while gtk::glib::MainContext::default().iteration(false) {}
                Task::none()
            }
            #[cfg(target_os = "macos")]
            Message::MacOsActivationTick => {
                if self.main_window.is_none() && crate::macos_app::activation_reopen_requested() {
                    Task::done(Message::RelaunchRequested)
                } else {
                    Task::none()
                }
            }
            Message::RelaunchRequested => {
                if self.main_window.is_none() {
                    Task::done(Message::ShowWindow)
                } else {
                    Task::none()
                }
            }
            Message::CertificateResetRequested(request) => {
                self.certificate_reset_queue.push_back(request);
                if self.main_window.is_none() {
                    Task::done(Message::ShowWindow)
                } else {
                    Task::none()
                }
            }
            Message::ConfirmCertificateReset => {
                self.respond_to_next_certificate_reset(true);
                Task::none()
            }
            Message::CancelCertificateReset => {
                self.respond_to_next_certificate_reset(false);
                Task::none()
            }
            Message::ShowWindow => {
                crate::macos_app::set_main_window_visible(true);
                if let Some(id) = self.main_window {
                    window::gain_focus(id)
                } else {
                    let (id, open_task) = window::open(defaults::default_window_settings());
                    self.main_window = Some(id);
                    open_task.discard()
                }
            }
            Message::HideWindow => {
                if let Some(id) = self.main_window {
                    self.cancel_pending_certificate_resets();
                    self.main_window = None;
                    crate::macos_app::set_main_window_visible(false);
                    window::close(id)
                } else {
                    Task::none()
                }
            }
            Message::Quit => {
                self.tray.take();
                std::process::exit(0);
            }
            Message::LoginWindowMessage(id, msg) => {
                if let Some(login_window) = self.login_windows.get_mut(&id) {
                    let task = login_window.update(msg.clone());

                    if matches!(
                        msg,
                        login_window::Message::LoginSuccess(_)
                            | login_window::Message::LoginCancel
                            | login_window::Message::TwoFactorCancel
                    ) {
                        self.login_windows.remove(&id);
                        self.account_store = Some(Self::init_account_store_sync());

                        if let ImpactorScreen::Settings(_) = self.current_screen {
                            self.current_screen =
                                ImpactorScreen::Settings(settings::SettingsScreen::new());
                        }

                        if self.pending_installation {
                            if matches!(msg, login_window::Message::LoginSuccess(_)) {
                                self.pending_installation = false;
                                return Task::batch(vec![
                                    window::close(id),
                                    Task::done(Message::InstallerScreen(
                                        package::Message::RequestInstallation,
                                    )),
                                ]);
                            }
                        }

                        return window::close(id);
                    }

                    task.map(move |msg| Message::LoginWindowMessage(id, msg))
                } else {
                    Task::none()
                }
            }
            Message::MainScreen(msg) => {
                if let ImpactorScreen::Main(ref mut screen) = self.current_screen {
                    let task = screen.update(msg.clone()).map(Message::MainScreen);

                    if let general::Message::NavigateToInstaller(package) = msg {
                        let mut options = SignerOptions::default();
                        package.load_into_signer_options(&mut options);

                        let update_device = package
                            .get_bundle_identifier()
                            .as_deref()
                            .and_then(|bundle_id| self.update_device_for_source(bundle_id));

                        if let Some(device) = update_device.as_ref() {
                            log::info!(
                                "Imported IPA matches an app on {}; starting direct update",
                                device.name
                            );
                            self.selected_device = Some(device.clone());
                            // Saving the newly signed bundle is mandatory for
                            // updates. Otherwise a later renewal would restore
                            // the older app copy from the refresh store.
                            options.refresh = true;
                        }

                        self.current_screen = ImpactorScreen::Installer(
                            package::PackageScreen::new(Some(package), options),
                        );

                        if update_device.is_some() {
                            return Task::done(Message::InstallerScreen(
                                package::Message::RequestInstallation,
                            ));
                        }
                    } else if let general::Message::NavigateToUtilities = msg {
                        let rppairing_enabled = match &self.current_screen {
                            ImpactorScreen::Utilities(screen) => screen.rppairing_enabled,
                            _ => false,
                        };
                        self.current_screen =
                            ImpactorScreen::Utilities(self.build_utilities_screen());
                        return Task::done(Message::UtilitiesScreen(
                            utilties::Message::RefreshApps(rppairing_enabled),
                        ));
                    } else if let general::Message::NavigateToSignedApps = msg {
                        self.navigate_to_screen(ImpactorScreenType::SignedApps);
                    }

                    task
                } else {
                    Task::none()
                }
            }
            Message::UtilitiesScreen(msg) => {
                // Remember the remote pairing switch, otherwise it would reset
                // every time this screen is rebuilt.
                if let utilties::Message::ToggleRPPairing(enabled) = &msg {
                    if let Some(store) = &mut self.account_store {
                        if let Err(err) = store.set_remote_pairing_sync(*enabled) {
                            log::error!("Failed to persist remote pairing setting: {err}");
                        }
                    }
                }

                if let ImpactorScreen::Utilities(ref mut screen) = self.current_screen {
                    screen.update(msg).map(Message::UtilitiesScreen)
                } else {
                    Task::none()
                }
            }
            Message::SignedAppsScreen(msg) => match msg {
                signed_apps::Message::Back => Task::done(Message::PreviousScreen),
                signed_apps::Message::Refresh { udid, app_path } => {
                    Task::done(Message::RefreshAppNow { udid, app_path })
                }
            },
            Message::SettingsScreen(msg) => {
                if let ImpactorScreen::Settings(ref mut screen) = self.current_screen {
                    match msg {
                        settings::Message::ShowLogin => {
                            let (login_window, task) = login_window::LoginWindow::new();
                            let id = login_window.window_id().unwrap();
                            self.login_windows.insert(id, login_window);
                            task.map(move |msg| Message::LoginWindowMessage(id, msg))
                        }
                        settings::Message::SelectAccount(index) => {
                            if let Some(store) = &mut self.account_store {
                                let mut emails: Vec<_> = store.accounts().keys().cloned().collect();
                                emails.sort();
                                if let Some(email) = emails.get(index) {
                                    let _ = store.account_select_sync(email);
                                }
                            }
                            Task::none()
                        }
                        settings::Message::RemoveAccount(index) => {
                            if let Some(store) = &mut self.account_store {
                                let mut emails: Vec<_> = store.accounts().keys().cloned().collect();
                                emails.sort();
                                if let Some(email) = emails.get(index) {
                                    let _ = store.accounts_remove_sync(email);
                                }
                            }
                            Task::none()
                        }
                        settings::Message::ExportP12 => {
                            if let Some(account) = self
                                .account_store
                                .as_ref()
                                .and_then(|s| s.selected_account().cloned())
                            {
                                std::thread::spawn(move || {
                                    let rt = tokio::runtime::Builder::new_current_thread()
                                        .enable_all()
                                        .build()
                                        .unwrap();

                                    let _ = rt.block_on(async move {
                                        crate::subscriptions::export_certificate(account).await
                                    });
                                });
                            }
                            Task::none()
                        }
                        settings::Message::ToggleAutoStart(enabled) => {
                            if let Err(err) = crate::startup::set_auto_start_enabled(enabled) {
                                log::error!("Failed to update auto-start: {err}");
                            }
                            Task::none()
                        }
                        settings::Message::FetchTeams(ref email) => {
                            if let Some(account_store) = &self.account_store {
                                if let Some(account) = account_store.accounts().get(email) {
                                    let account_clone = account.clone();
                                    let email_clone = email.clone();

                                    return Task::perform(
                                        async move {
                                            let (tx, rx) = std::sync::mpsc::channel();

                                            std::thread::spawn(move || {
                                                let rt = tokio::runtime::Runtime::new().unwrap();
                                                let result = rt.block_on(async move {
                                                    crate::subscriptions::fetch_teams(
                                                        &account_clone,
                                                    )
                                                    .await
                                                    .unwrap_or_else(|e| {
                                                        log::error!("Failed to fetch teams: {}", e);
                                                        Vec::new()
                                                    })
                                                });
                                                let _ = tx.send(result);
                                            });

                                            rx.recv().unwrap_or_default()
                                        },
                                        move |teams| {
                                            Message::SettingsScreen(settings::Message::TeamsLoaded(
                                                email_clone,
                                                teams,
                                            ))
                                        },
                                    );
                                }
                            }
                            screen.update(msg).map(Message::SettingsScreen)
                        }
                        settings::Message::SelectTeam(ref email, ref team_id) => {
                            if let Some(store) = &mut self.account_store {
                                if let Err(e) =
                                    store.update_account_team_sync(email, team_id.clone())
                                {
                                    log::error!("Failed to update team: {:?}", e);
                                } else {
                                    self.account_store = Some(Self::init_account_store_sync());
                                }
                            }
                            screen.update(msg).map(Message::SettingsScreen)
                        }
                        settings::Message::SelectLocale(choice) => {
                            self.selected_locale = choice.clone();
                            // "System" means: follow the system locale, normalised
                            // onto the locale files we actually ship (and Chinese
                            // by default, matching the app's default language).
                            let effective = match &choice {
                                Some(code) => code.clone(),
                                None => {
                                    let system =
                                        current_locale::current_locale().unwrap_or_default();
                                    plume_utils::locale::normalize_locale(
                                        &system,
                                        rust_i18n::available_locales!(),
                                        plume_utils::locale::FALLBACK_LOCALE,
                                    )
                                }
                            };
                            rust_i18n::set_locale(&effective);
                            if let Some(store) = &mut self.account_store {
                                if let Err(err) = store.set_locale_sync(choice) {
                                    log::error!("Failed to persist locale: {err}");
                                }
                            }
                            Task::none()
                        }
                        settings::Message::SelectRefreshDays(days) => {
                            if let Some(store) = &mut self.account_store {
                                if let Err(err) = store.set_refresh_days_before_sync(days) {
                                    log::error!("Failed to persist refresh days: {err}");
                                }
                            }
                            screen.update(msg).map(Message::SettingsScreen)
                        }
                        settings::Message::OpenLogFolder => {
                            let path = crate::defaults::get_data_path();
                            if let Err(err) = open::that(&path) {
                                log::error!("Failed to open {}: {err}", path.display());
                            }
                            Task::none()
                        }
                        settings::Message::SelfCheck => {
                            let update = screen.update(msg).map(Message::SettingsScreen);
                            let check = Task::perform(
                                crate::diagnostics::write_self_check_report(),
                                |report| {
                                    Message::SettingsScreen(settings::Message::SelfCheckFinished(
                                        report,
                                    ))
                                },
                            );
                            Task::batch([update, check])
                        }
                        settings::Message::InstallDrivers => {
                            if let Err(err) = crate::diagnostics::install_drivers_elevated() {
                                log::error!("Failed to start the driver installer: {err}");
                            }
                            Task::none()
                        }
                        _ => screen.update(msg).map(Message::SettingsScreen),
                    }
                } else {
                    Task::none()
                }
            }
            Message::InstallerScreen(msg) => {
                if let ImpactorScreen::Installer(ref mut screen) = self.current_screen {
                    match msg {
                        package::Message::Back => Task::done(Message::PreviousScreen),
                        package::Message::RequestInstallation => {
                            if screen.selected_package.is_none() {
                                return Task::none();
                            }

                            use plume_utils::SignerMode;
                            if matches!(screen.options.mode, SignerMode::Pem) {
                                let bundle_id = screen
                                    .selected_package
                                    .as_ref()
                                    .and_then(|package| package.get_bundle_identifier());
                                if self
                                    .account_store
                                    .as_ref()
                                    .and_then(|store| {
                                        store.account_for_new_app(bundle_id.as_deref())
                                    })
                                    .is_none()
                                {
                                    self.pending_installation = true;

                                    let (login_window, task) = login_window::LoginWindow::new();
                                    let id = login_window.window_id().unwrap();
                                    self.login_windows.insert(id, login_window);
                                    return task
                                        .map(move |msg| Message::LoginWindowMessage(id, msg));
                                }
                            }

                            self.start_installation_task()
                        }
                        _ => screen.update(msg).map(Message::InstallerScreen),
                    }
                } else {
                    Task::none()
                }
            }
            Message::ProgressScreen(msg) => {
                if let ImpactorScreen::Progress(ref mut screen) = self.current_screen {
                    match msg {
                        progress::Message::Back => Task::done(Message::PreviousScreen),
                        progress::Message::InstallationFinished => {
                            Task::done(Message::UpdateTrayMenu)
                        }
                        _ => screen.update(msg).map(Message::ProgressScreen),
                    }
                } else {
                    Task::none()
                }
            }
            Message::RefreshAppNow { udid, app_path } => {
                let key = refresh_key(&udid, &app_path);
                if self.refreshing_apps.contains(&key) {
                    return Task::none();
                }

                let Some(store) = self.account_store.clone() else {
                    return Task::none();
                };
                let Some(daemon_devices) = REFRESH_DAEMON_DEVICES.get().cloned() else {
                    return Task::none();
                };

                self.refreshing_apps.insert(key.clone());
                self.refresh_results.remove(&key);

                let completed_udid = udid.clone();
                let completed_path = app_path.clone();
                Task::perform(
                    async move {
                        let (tx, rx) = std::sync::mpsc::channel();
                        std::thread::spawn(move || {
                            let result = match tokio::runtime::Builder::new_current_thread()
                                .enable_all()
                                .build()
                            {
                                Ok(runtime) => runtime.block_on(run_manual_refresh(
                                    store,
                                    daemon_devices,
                                    udid,
                                    app_path,
                                )),
                                Err(error) => {
                                    Err(format!("Failed to start the renewal runtime: {error}"))
                                }
                            };
                            let _ = tx.send(result);
                        });

                        rx.recv().unwrap_or_else(|_| {
                            Err("The renewal worker stopped unexpectedly".to_string())
                        })
                    },
                    move |result| Message::RefreshAppCompleted {
                        udid: completed_udid,
                        app_path: completed_path,
                        result,
                    },
                )
            }
            Message::RefreshAppCompleted {
                udid,
                app_path,
                result,
            } => {
                let key = refresh_key(&udid, &app_path);
                self.refreshing_apps.remove(&key);

                if let Err(error) = &result {
                    log::error!("Manual refresh failed for {app_path} on {udid}: {error}");
                    notify_rust::Notification::new()
                        .summary(crate::APP_NAME)
                        .body(&format!("续签失败：{error}"))
                        .show()
                        .ok();
                }
                self.refresh_results.insert(key, result);
                self.account_store = Some(Self::init_account_store_sync());

                if let (Some(store), Some(tray)) = (&self.account_store, &mut self.tray) {
                    tray.update_refresh_apps(store);
                }
                Task::none()
            }
            Message::ForgetApp { udid, app_path } => {
                if let Some(store) = &mut self.account_store {
                    if let Some(mut refresh_device) = store.get_refresh_device(&udid).cloned() {
                        if let Some(app) = refresh_device
                            .apps
                            .iter()
                            .find(|a| a.path.to_string_lossy() == app_path)
                        {
                            let app_path_buf = app.path.clone();
                            std::thread::spawn(move || {
                                if app_path_buf.exists() {
                                    if let Err(e) = std::fs::remove_dir_all(&app_path_buf) {
                                        log::error!(
                                            "Failed to delete app at {:?}: {}",
                                            app_path_buf,
                                            e
                                        );
                                    } else {
                                        log::info!("Deleted app at {:?}", app_path_buf);
                                    }
                                }
                            });
                        }

                        refresh_device
                            .apps
                            .retain(|a| a.path.to_string_lossy() != app_path);

                        if refresh_device.apps.is_empty() {
                            let _ = store.remove_refresh_device_sync(&udid);
                        } else {
                            let _ = store.add_or_update_refresh_device_sync(refresh_device);
                        }

                        self.account_store = Some(Self::init_account_store_sync());
                    }
                }
                Task::done(Message::UpdateTrayMenu)
            }
            Message::UpdateTrayMenu => {
                self.account_store = Some(Self::init_account_store_sync());

                if let Some(store) = &self.account_store {
                    match &mut self.tray {
                        Some(existing_tray) => {
                            existing_tray.update_refresh_apps(&store);
                        }
                        None => {
                            let mut new_tray = ImpactorTray::new();
                            new_tray.update_refresh_apps(&store);
                            self.tray = Some(new_tray);
                        }
                    }
                }
                Task::none()
            }
            Message::StartInstallation => Task::none(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let device_subscription = subscriptions::device_listener();

        let tray_subscription = subscriptions::tray_subscription();

        let hover_subscription = if let ImpactorScreen::Main(_) = self.current_screen {
            subscriptions::file_hover_subscription()
        } else {
            Subscription::none()
        };

        let progress_subscription =
            if let ImpactorScreen::Progress(ref progress) = self.current_screen {
                subscriptions::installation_progress_listener(progress.progress_rx.clone()).map(
                    |(status, progress_val)| {
                        Message::ProgressScreen(progress::Message::InstallationProgress(
                            status,
                            progress_val,
                        ))
                    },
                )
            } else {
                Subscription::none()
            };

        let tray_menu_refresh_subscription = subscriptions::tray_menu_refresh_subscription();
        let certificate_reset_subscription = subscriptions::certificate_reset_subscription();
        let relaunch_subscription = subscriptions::relaunch_subscription();

        let close_subscription = iced::event::listen_with(|event, _status, _id| {
            if let iced::Event::Window(window::Event::CloseRequested) = event {
                return Some(Message::HideWindow);
            }
            None
        });

        Subscription::batch(vec![
            device_subscription,
            tray_subscription,
            hover_subscription,
            progress_subscription,
            tray_menu_refresh_subscription,
            certificate_reset_subscription,
            relaunch_subscription,
            close_subscription,
        ])
    }

    pub fn view(&self, window_id: window::Id) -> Element<'_, Message> {
        if let Some(login_window) = self.login_windows.get(&window_id) {
            return login_window
                .view()
                .map(move |msg| Message::LoginWindowMessage(window_id, msg));
        }

        let has_device = self.selected_device.is_some();
        let screen_content = self.view_current_screen(has_device);
        let top_bar = self.view_top_bar();
        let base: Element<'_, Message> =
            container(column(vec![top_bar, screen_content]).spacing(appearance::THEME_PADDING))
                .padding(appearance::THEME_PADDING)
                .into();

        if self.certificate_reset_queue.front().is_some() {
            stack![base, self.view_certificate_reset_prompt()].into()
        } else {
            base
        }
    }

    fn view_current_screen(&self, has_device: bool) -> Element<'_, Message> {
        match &self.current_screen {
            ImpactorScreen::Main(screen) => screen.view().map(Message::MainScreen),
            ImpactorScreen::Utilities(screen) => screen.view().map(Message::UtilitiesScreen),
            ImpactorScreen::SignedApps(screen) => screen
                .view(
                    &self.account_store,
                    &self.refreshing_apps,
                    &self.refresh_results,
                )
                .map(Message::SignedAppsScreen),
            ImpactorScreen::Settings(screen) => screen
                .view(&self.account_store, &self.selected_locale)
                .map(Message::SettingsScreen),
            ImpactorScreen::Installer(screen) => {
                screen.view(has_device).map(Message::InstallerScreen)
            }
            ImpactorScreen::Progress(screen) => screen.view().map(Message::ProgressScreen),
        }
    }

    fn view_top_bar(&self) -> Element<'_, Message> {
        let device_names: Vec<String> = self.devices.iter().map(|d| d.to_string()).collect();
        let selected_device_name = self.selected_device.as_ref().map(|d| d.to_string());
        let placeholder_str = selected_device_name
            .as_ref()
            .map(String::as_str)
            .unwrap_or("No Device");

        let right_button = if matches!(self.current_screen, ImpactorScreen::Settings(_)) {
            button(appearance::icon(appearance::CHEVRON_BACK))
                .on_press(Message::PreviousScreen)
                .style(appearance::s_button)
        } else if matches!(
            self.current_screen,
            ImpactorScreen::Utilities(_) | ImpactorScreen::SignedApps(_)
        ) {
            button(appearance::icon(appearance::CHEVRON_BACK))
                .on_press(Message::PreviousScreen)
                .style(appearance::s_button)
        } else {
            button(appearance::icon(appearance::GEAR))
                .style(appearance::s_button)
                .on_press(Message::NavigateToScreen(ImpactorScreenType::Settings))
        };

        container(
            row![
                container(text("")).width(Fill),
                pick_list(
                    device_names,
                    selected_device_name.clone(),
                    Message::ComboBoxSelected
                )
                .style(appearance::s_pick_list)
                .placeholder(placeholder_str)
                .width(250),
                right_button
            ]
            .spacing(appearance::THEME_PADDING),
        )
        .width(Fill)
        .into()
    }

    fn view_certificate_reset_prompt(&self) -> Element<'_, Message> {
        let Some(request) = self.certificate_reset_queue.front() else {
            return container(text("")).into();
        };

        let actions = row![
            button(text("Cancel"))
                .on_press(Message::CancelCertificateReset)
                .style(appearance::s_button),
            button(text("Continue"))
                .on_press(Message::ConfirmCertificateReset)
                .style(appearance::p_button),
        ]
        .spacing(appearance::THEME_PADDING);

        let dialog = container(
            column![
                text("Certificate reset required").size(appearance::THEME_FONT_SIZE + 2.0),
                text(&request.message),
                actions,
            ]
            .spacing(appearance::THEME_PADDING),
        )
        .padding(appearance::THEME_PADDING * 2.0)
        .max_width(420.0)
        .style(|theme: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(theme.palette().background)),
            border: iced::Border {
                width: 1.0,
                color: theme.palette().warning,
                radius: appearance::THEME_CORNER_RADIUS.into(),
            },
            ..Default::default()
        });

        container(dialog)
            .width(Fill)
            .height(Fill)
            .center(Fill)
            .style(|_theme: &iced::Theme| container::Style {
                background: Some(iced::Background::Color(iced::Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 0.45,
                })),
                ..Default::default()
            })
            .into()
    }

    fn navigate_to_screen(&mut self, screen_type: ImpactorScreenType) {
        match screen_type {
            ImpactorScreenType::Main => {
                if let ImpactorScreen::Installer(installer) = &self.current_screen {
                    if let Some(package) = installer.selected_package.clone() {
                        package.remove_package_stage();
                    }
                }

                self.current_screen = ImpactorScreen::Main(general::GeneralScreen::new());
            }
            ImpactorScreenType::Utilities => {
                self.current_screen = ImpactorScreen::Utilities(self.build_utilities_screen());
            }
            ImpactorScreenType::SignedApps => {
                if let Some(store) = &mut self.account_store {
                    crate::macos_signed_apps::sync_installed_apps(store);
                }
                self.current_screen =
                    ImpactorScreen::SignedApps(signed_apps::SignedAppsScreen::new());
            }
            ImpactorScreenType::Settings => {
                self.current_screen = ImpactorScreen::Settings(settings::SettingsScreen::new());
            }
            ImpactorScreenType::Progress => {
                self.current_screen = ImpactorScreen::Progress(progress::ProgressScreen::new());
            }
            _ => {}
        }
    }

    fn start_installation_task(&mut self) -> Task<Message> {
        if let ImpactorScreen::Installer(installer) = &self.current_screen {
            let Some(package) = installer.selected_package.clone() else {
                return Task::none();
            };

            let device = self.selected_device.clone();
            let options = installer.options.clone();
            let account = self.account_store.as_ref().and_then(|store| {
                // Spread apps over all added Apple IDs automatically: each
                // free account may only sign three apps at a time.
                let bundle_id = installer
                    .selected_package
                    .as_ref()
                    .and_then(|package| package.get_bundle_identifier());

                store.account_for_new_app(bundle_id.as_deref())
            });
            let mut store = self.account_store.clone();

            let (tx, rx) = std::sync::mpsc::channel();
            let progress_rx = std::sync::Arc::new(std::sync::Mutex::new(rx));

            let mut progress_screen = progress::ProgressScreen::new();
            progress_screen.start_installation(progress_rx.clone());
            self.current_screen = ImpactorScreen::Progress(progress_screen);

            std::thread::spawn(move || {
                let rt = tokio::runtime::Runtime::new().unwrap();
                let tx_error = tx.clone();
                rt.block_on(async move {
                    match subscriptions::run_installation(
                        &package,
                        device.as_ref(),
                        &options,
                        account.as_ref(),
                        store.as_mut(),
                        &tx,
                    )
                    .await
                    {
                        Ok(_) => {
                            let _ = tx.send(("Installation complete!".to_string(), 100));

                            if std::env::var("PLUME_DELETE_AFTER_FINISHED").is_err() {
                                package.remove_package_stage();
                            }
                        }
                        Err(e) => {
                            let _ = tx_error.send((format!("Error: {}", e), -1));

                            if std::env::var("PLUME_DELETE_AFTER_FINISHED").is_err() {
                                package.remove_package_stage();
                            }
                        }
                    }
                });
            });

            Task::none()
        } else {
            Task::none()
        }
    }

    fn update_device_for_source(&self, bundle_id: &str) -> Option<Device> {
        let store = self.account_store.as_ref()?;

        if let Some(selected) = &self.selected_device {
            if store.has_refresh_app_for_source(&selected.udid, bundle_id) {
                return Some(selected.clone());
            }
        }

        let mut matches = self
            .devices
            .iter()
            .filter(|device| store.has_refresh_app_for_source(&device.udid, bundle_id));
        let first = matches.next()?.clone();

        // When two connected devices contain the same app, do not guess which
        // one the user intended. The normal installer remains available and
        // shows the device selector.
        matches.next().is_none().then_some(first)
    }
}

pub(super) fn refresh_key(udid: &str, app_path: &str) -> String {
    format!("{udid}\u{1f}{app_path}")
}

async fn run_signed_app_scan(store: AccountStore, device: Device) -> Result<AccountStore, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime.block_on(crate::device_signed_apps::sync_installed_apps(
                store, device,
            )),
            Err(error) => Err(format!("Failed to start the device scan runtime: {error}")),
        };
        let _ = tx.send(result);
    });
    rx.recv()
        .unwrap_or_else(|_| Err("The device scan worker stopped unexpectedly".to_string()))
}

async fn run_manual_refresh(
    store: AccountStore,
    daemon_devices: crate::refresh::ConnectedDevices,
    udid: String,
    app_path: String,
) -> Result<(), String> {
    let refresh_device = store
        .get_refresh_device(&udid)
        .cloned()
        .ok_or_else(|| "The signing record no longer exists".to_string())?;
    let app = refresh_device
        .apps
        .iter()
        .find(|app| app.path.to_string_lossy() == app_path)
        .cloned()
        .ok_or_else(|| "The app is no longer in the signing list".to_string())?;

    let device = if refresh_device.is_mac {
        // A local Mac is not a usbmuxd device. Construct it directly so manual
        // renewal never waits for the iPhone device listener to publish it.
        Device::local_mac(refresh_device.name.clone(), udid.clone(), u32::MAX)
    } else {
        let started = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(60);
        loop {
            if let Ok(devices) = daemon_devices.lock() {
                if let Some(device) = devices.get(&udid) {
                    break device.clone();
                }
            }
            if started.elapsed() >= timeout {
                return Err("Device is not reachable over USB or Wi-Fi".to_string());
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    };

    log::info!(
        "Manual refresh requested for {:?} on {}",
        app.path,
        refresh_device.name
    );
    crate::refresh::RefreshDaemon::new()
        .refresh_app_manual(&store, &refresh_device, &app, &device)
        .await
}
