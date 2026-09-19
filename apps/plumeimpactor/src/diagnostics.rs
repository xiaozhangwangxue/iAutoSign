//! One-click environment self-check.
//!
//! When something does not work on a user's PC this produces a readable report
//! showing which part failed: the Apple device service, the bundled anisette
//! libraries, the stored account/session, or the registered auto-renewals.

use std::fmt::Write as _;

use crate::defaults::get_data_path;

/// Runs the self-check and also stores the result as `selfcheck.txt` in the data
/// directory, so the user can simply send that file (or find it even when the
/// window could not be opened).
pub async fn write_self_check_report() -> String {
    let report = run_self_check().await;
    let path = get_data_path().join("selfcheck.txt");

    if let Err(err) = std::fs::write(&path, &report) {
        log::warn!("Could not write {}: {err}", path.display());
    } else {
        log::info!("Self-check report written to {}", path.display());
    }

    report
}

pub async fn run_self_check() -> String {
    let mut report = String::new();

    let data_path = get_data_path();
    let _ = writeln!(report, "AutoSign 环境自检 / self-check");
    let _ = writeln!(report, "版本 version: {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(report, "数据目录 data dir: {}", data_path.display());
    let _ = writeln!(
        report,
        "日志文件 log file: {}",
        data_path.join("autosign.log").display()
    );
    let _ = writeln!(report);

    // 1) The libraries that generate anisette data locally.
    let libraries_present = plume_core::auth::anisette_libraries_present(data_path.clone());
    let _ = writeln!(
        report,
        "1) 本地签名数据组件 / local anisette libraries: {}",
        if libraries_present {
            "文件存在 present"
        } else {
            "缺失 missing"
        }
    );

    if libraries_present {
        let _ = writeln!(report, "   加载测试 load test: {}", probe_anisette());
    }
    let _ = writeln!(report);

    // 2) Apple's device service and whatever it can currently see.
    let _ = writeln!(report, "2) Apple 驱动服务与设备 / device service:");
    match idevice::usbmuxd::UsbmuxdConnection::default().await {
        Ok(mut connection) => match connection.get_devices().await {
            Ok(devices) => {
                let _ = writeln!(
                    report,
                    "   服务可达 service reachable; 发现 {} 台设备",
                    devices.len()
                );
                for device in devices {
                    let kind = match device.connection_type {
                        idevice::usbmuxd::Connection::Usb => "USB",
                        idevice::usbmuxd::Connection::Network(_) => "Wi-Fi",
                        idevice::usbmuxd::Connection::Unknown(_) => "unknown",
                    };
                    let _ = writeln!(report, "   - {} ({})", device.udid, kind);
                }
            }
            Err(err) => {
                let _ = writeln!(report, "   服务可达但读取设备失败: {err}");
            }
        },
        Err(err) => {
            let _ = writeln!(report, "   无法连接驱动服务 cannot reach the service: {err}");
            let _ = writeln!(report, "   （通常是驱动没装或服务没启动）");
        }
    }
    let _ = writeln!(report);

    // 3) Accounts and scheduled renewals.
    let _ = writeln!(report, "3) 账号与自动续签 / accounts & renewals:");
    let store_path = data_path.join("accounts.json");

    match plume_store::AccountStore::load(&Some(store_path)).await {
        Ok(store) => {
            let _ = writeln!(report, "   已添加账号 accounts: {}", store.accounts().len());

            if let Some(selected) = store.selected_account() {
                let _ = writeln!(
                    report,
                    "   当前选中 selected: {} (team: {})",
                    selected.email(),
                    if selected.team_id().is_empty() {
                        "未选择 none"
                    } else {
                        selected.team_id()
                    }
                );
            }

            let _ = writeln!(
                report,
                "   提前续签天数 days before expiry: {}",
                store.refresh_days_before()
            );

            let refreshes = store.refreshes();
            let _ = writeln!(report, "   自动续签设备 devices: {}", refreshes.len());
            for (udid, device) in refreshes {
                let _ = writeln!(
                    report,
                    "   - {} / {}: {} 个 App",
                    udid,
                    device.name,
                    device.apps.len()
                );
                for app in &device.apps {
                    let _ = writeln!(
                        report,
                        "       · {}（下次续签 next: {}）",
                        app.name.clone().unwrap_or_else(|| "?".to_string()),
                        app.scheduled_refresh.format("%Y-%m-%d %H:%M UTC")
                    );
                }
            }
        }
        Err(err) => {
            let _ = writeln!(report, "   读取账号数据失败: {err}");
        }
    }

    let _ = writeln!(report);
    let _ = writeln!(report, "把这段内容连同 autosign.log 一起发我即可定位问题。");

    report
}

/// Runs the library loading test in a separate process: the bundled Apple
/// libraries run native code, so a problem there must not take down the UI.
fn probe_anisette() -> String {
    let Ok(exe) = std::env::current_exe() else {
        return "无法定位程序路径".to_string();
    };

    let mut command = std::process::Command::new(exe);
    command.arg("--probe-anisette");

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }

    match command.output() {
        Ok(output) if output.status.success() => "成功（本地 anisette 可用）".to_string(),
        Ok(output) => format!("失败（退出码 {:?}，详见日志）", output.status.code()),
        Err(err) => format!("无法启动自检进程: {err}"),
    }
}
