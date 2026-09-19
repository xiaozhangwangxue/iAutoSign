//! Environment self-check.
//!
//! Lives in this crate (rather than in the GUI) so that it can be tested
//! without opening a window and reused by the command line tool.

use std::fmt::Write as _;
use std::path::Path;
use std::path::PathBuf;

/// Builds the human readable report.
///
/// `anisette_load_result` is the outcome of loading the bundled Apple libraries
/// in a separate process (loading runs native code, so the caller isolates it);
/// `None` means the check was skipped.
pub async fn run_self_check(data_path: PathBuf, anisette_load_result: Option<String>) -> String {
    let mut report = String::new();

    let _ = writeln!(report, "AutoSign 环境自检 / self-check");
    let _ = writeln!(report, "版本 version: {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(report, "数据目录 data dir: {}", data_path.display());
    let _ = writeln!(
        report,
        "日志文件 log file: {}",
        data_path.join("autosign.log").display()
    );
    let _ = writeln!(report);

    // 1) Libraries that generate anisette data locally.
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

    if let Some(result) = anisette_load_result {
        let _ = writeln!(report, "   加载测试 load test: {result}");
    }
    let _ = writeln!(report);

    // 2) Apple's device service and what it can currently see.
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

/// Builds the PowerShell command that re-runs a script with administrator
/// rights, quoting the paths so that directories containing spaces (or quotes)
/// keep working.
pub fn powershell_elevation_command(script: &Path, log_path: &Path) -> String {
    let inner = format!(
        "-NoProfile -ExecutionPolicy Bypass -File \"{}\" -LogPath \"{}\"",
        script.display(),
        log_path.display()
    );

    format!(
        "Start-Process -FilePath 'powershell.exe' -ArgumentList '{}' -Verb RunAs",
        inner.replace('\'', "''")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let id = uuid::Uuid::new_v4().simple().to_string();
        let directory = std::env::temp_dir().join(format!("plume_diag_{}", &id[..8]));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[tokio::test]
    async fn report_covers_every_section() {
        let directory = temp_dir();

        let report = run_self_check(directory.clone(), Some("skipped in tests".to_string())).await;

        assert!(report.contains("AutoSign 环境自检"), "{report}");
        assert!(report.contains("1) 本地签名数据组件"), "{report}");
        assert!(report.contains("2) Apple 驱动服务与设备"), "{report}");
        assert!(report.contains("3) 账号与自动续签"), "{report}");
        assert!(report.contains("load test: skipped in tests"), "{report}");

        // A fresh data directory means: no accounts, default renewal window.
        assert!(report.contains("accounts: 0"), "{report}");
        assert!(report.contains("days before expiry: 3"), "{report}");
        assert!(report.contains("devices: 0"), "{report}");

        std::fs::remove_dir_all(&directory).ok();
    }

    /// When no Apple service is present (CI, a PC without drivers, this test)
    /// the report must say so instead of failing or panicking.
    #[tokio::test]
    async fn missing_device_service_is_only_reported() {
        let directory = temp_dir();

        let report = run_self_check(directory.clone(), None).await;

        assert!(
            report.contains("服务可达 service reachable")
                || report.contains("无法连接驱动服务 cannot reach the service"),
            "{report}"
        );

        std::fs::remove_dir_all(&directory).ok();
    }

    #[test]
    fn elevation_command_quotes_paths_with_spaces() {
        let command = powershell_elevation_command(
            Path::new(r"C:\Program Files\AutoSign\install-drivers.ps1"),
            Path::new(r"C:\Users\Some User\AppData\Local\Temp\autosign-deps.log"),
        );

        assert!(command.contains("-Verb RunAs"), "{command}");
        assert!(
            command.contains(r#""C:\Program Files\AutoSign\install-drivers.ps1""#),
            "{command}"
        );
        assert!(
            command.contains(r#"-LogPath "C:\Users\Some User\AppData\Local\Temp\autosign-deps.log""#),
            "{command}"
        );
    }

    #[test]
    fn elevation_command_escapes_single_quotes() {
        let command = powershell_elevation_command(
            Path::new(r"C:\Users\O'Brien\AutoSign\install-drivers.ps1"),
            Path::new(r"C:\Temp\autosign-deps.log"),
        );

        // A lone quote would terminate the argument list early.
        assert!(command.contains("O''Brien"), "{command}");
        assert_eq!(command.matches("-Verb RunAs").count(), 1, "{command}");
    }
}
