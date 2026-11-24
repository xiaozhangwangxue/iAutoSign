use std::collections::{HashMap, HashSet};

use chrono::{Duration, Local};
use iced::widget::{button, column, container, row, rule, scrollable, text};
use iced::{Center, Color, Element, Fill};
use plume_store::AccountStore;
use rust_i18n::t;

use crate::appearance;

#[derive(Debug, Clone)]
pub enum Message {
    Back,
    Refresh { udid: String, app_path: String },
}

#[derive(Debug, Clone, Default)]
pub struct SignedAppsScreen;

impl SignedAppsScreen {
    pub fn new() -> Self {
        Self
    }

    pub fn view<'a>(
        &'a self,
        store: &'a Option<AccountStore>,
        refreshing: &'a HashSet<String>,
        results: &'a HashMap<String, Result<(), String>>,
    ) -> Element<'a, Message> {
        let mut content = column![
            text(t!("signed_apps_title")).size(appearance::THEME_FONT_SIZE + 6.0),
            text(t!("signed_apps_desc"))
                .size(14)
                .color(Color::from_rgb(0.65, 0.65, 0.65)),
        ]
        .spacing(appearance::THEME_PADDING);

        let mut app_count = 0;
        if let Some(store) = store {
            for (udid, device) in store.refreshes() {
                if device.apps.is_empty() {
                    continue;
                }

                content = content.push(
                    column![
                        container(rule::horizontal(1)).padding([appearance::THEME_PADDING, 0.0]),
                        text(&device.name).size(appearance::THEME_FONT_SIZE + 1.0),
                    ]
                    .spacing(4),
                );

                for app in &device.apps {
                    app_count += 1;
                    let app_path = app.path.to_string_lossy().to_string();
                    let key = super::refresh_key(udid, &app_path);
                    let is_refreshing = refreshing.contains(&key);
                    let account_email = app.account.as_deref().unwrap_or(&device.account);
                    let has_account =
                        !account_email.is_empty() && store.get_account(account_email).is_some();
                    let source_exists = app.path.exists();
                    let can_refresh = has_account && source_exists && !is_refreshing;
                    let expiry =
                        app.scheduled_refresh + Duration::days(store.refresh_days_before() as i64);

                    let mode = if app.auto_refresh {
                        t!("signed_apps_auto")
                    } else {
                        t!("signed_apps_manual")
                    };
                    let date_label = if app.auto_refresh {
                        t!(
                            "signed_apps_next_refresh",
                            date = app
                                .scheduled_refresh
                                .with_timezone(&Local)
                                .format("%Y-%m-%d %H:%M")
                                .to_string()
                        )
                    } else {
                        t!(
                            "signed_apps_expiry",
                            date = expiry
                                .with_timezone(&Local)
                                .format("%Y-%m-%d %H:%M")
                                .to_string()
                        )
                    };
                    let button_label = if is_refreshing {
                        t!("signed_apps_refreshing")
                    } else {
                        t!("signed_apps_refresh_now")
                    };

                    let mut app_card = column![
                        row![
                            column![
                                text(app.name.as_deref().unwrap_or("???")).size(16),
                                text(app.bundle_id.as_deref().unwrap_or("???"))
                                    .size(12)
                                    .color(Color::from_rgb(0.58, 0.58, 0.58)),
                                text(format!("{} · {}", mode, date_label))
                                    .size(13)
                                    .color(Color::from_rgb(0.72, 0.72, 0.72)),
                            ]
                            .spacing(2)
                            .width(Fill),
                            button(text(button_label).align_x(Center))
                                .on_press_maybe(can_refresh.then(|| Message::Refresh {
                                    udid: udid.clone(),
                                    app_path: app_path.clone(),
                                }))
                                .style(appearance::p_button),
                        ]
                        .spacing(appearance::THEME_PADDING)
                        .align_y(Center),
                    ]
                    .spacing(5);

                    if !has_account {
                        app_card = app_card.push(
                            text(t!("signed_apps_account_missing"))
                                .size(13)
                                .color(Color::from_rgb(0.95, 0.65, 0.25)),
                        );
                    } else if !source_exists {
                        app_card = app_card.push(
                            text(t!("signed_apps_source_missing"))
                                .size(13)
                                .color(Color::from_rgb(0.95, 0.45, 0.4)),
                        );
                    }

                    if let Some(result) = results.get(&key) {
                        match result {
                            Ok(()) => {
                                app_card = app_card.push(
                                    text(t!("signed_apps_refresh_success"))
                                        .size(13)
                                        .color(Color::from_rgb(0.35, 0.8, 0.45)),
                                );
                            }
                            Err(error) => {
                                app_card = app_card.push(
                                    text(t!("signed_apps_refresh_failed", error = error))
                                        .size(13)
                                        .color(Color::from_rgb(0.95, 0.45, 0.4)),
                                );
                            }
                        }
                    }

                    content = content.push(container(app_card).padding([8.0, 4.0]));
                }
            }
        }

        if app_count == 0 {
            content = content.push(
                container(text(t!("signed_apps_empty")).color(Color::from_rgb(0.7, 0.7, 0.7)))
                    .center(Fill)
                    .height(Fill),
            );
        }

        column![
            scrollable(content).height(Fill),
            button(text(t!("back")).align_x(Center))
                .on_press(Message::Back)
                .width(Fill)
                .style(appearance::s_button),
        ]
        .spacing(appearance::THEME_PADDING)
        .into()
    }
}
