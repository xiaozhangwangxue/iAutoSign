//! GUI glue for the environment self-check.
//!
//! The report itself is built in `plume_utils` so it can be tested and reused;
//! this module adds the two things that need the application binary: an isolated
//! library loading test and writing `selfcheck.txt` next to the log.

use crate::defaults::get_data_path;

/// Runs the self-check and stores the result as `selfcheck.txt`, so the user can
/// simply send that file (and can find it even when the window did not open).
pub async fn write_self_check_report() -> String {
    let probe = probe_anisette();
    let report = plume_utils::diagnostics::run_self_check(get_data_path(), Some(probe)).await;

    let path = get_data_path().join("selfcheck.txt");

    if let Err(err) = std::fs::write(&path, &report) {
        log::warn!("Could not write {}: {err}", path.display());
    } else {
        log::info!("Self-check report written to {}", path.display());
    }

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

/// Path of the driver installer that ships next to the application.
fn driver_script() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let directory = exe.parent()?;

    ["install-drivers.ps1", "deps.ps1"]
        .iter()
        .map(|name| directory.join(name))
        .find(|path| path.exists())
}

/// True when this build can install the Apple drivers on demand.
pub fn can_install_drivers() -> bool {
    cfg!(target_os = "windows") && driver_script().is_some()
}

/// Re-runs the bundled driver installer with administrator rights.
///
/// Used when the drivers were missing or failed to install during setup: the
/// user can repair it from inside the application instead of re-running the
/// whole installer.
#[cfg(target_os = "windows")]
pub fn install_drivers_elevated() -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let script = driver_script().ok_or("deps.ps1 未找到（请重新运行安装包）")?;

    // Same log location the installer uses, so the user always finds one file.
    let log_path = std::env::var("ProgramData")
        .map(|base| std::path::PathBuf::from(base).join("AutoSign").join("logs"))
        .unwrap_or_else(|_| std::env::temp_dir());
    let _ = std::fs::create_dir_all(&log_path);
    let log_path = log_path.join("deps-install.log");

    // The installer ships Apple's packages next to the application, which lets
    // the repair work without internet access.
    let bundle_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|parent| parent.join("drivers")))
        .filter(|directory| directory.exists());

    let command = plume_utils::diagnostics::powershell_elevation_command(
        &script,
        &log_path,
        bundle_dir.as_deref(),
    );

    log::info!("Requesting elevated driver installation via: {command}");

    std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"])
        .arg(command)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("无法启动驱动安装程序: {err}"))
}

#[cfg(not(target_os = "windows"))]
pub fn install_drivers_elevated() -> Result<(), String> {
    Err("驱动安装仅在 Windows 上可用".to_string())
}
