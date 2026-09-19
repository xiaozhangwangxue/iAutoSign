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
