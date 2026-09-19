//! Turns the opaque `installd` error codes iOS returns into something a user
//! can act on.

/// Adds a short explanation and next step to a failed installation message.
///
/// Returns the original message unchanged when the code is not recognised.
pub fn enrich_install_error(error: &str) -> String {
    let Some(hint) = explain(error) else {
        return error.to_string();
    };

    format!("{error}\n\n{hint}")
}

fn explain(error: &str) -> Option<&'static str> {
    let hint = if error.contains("0xe8008015") {
        "含义：iOS 认为这个 App 没有匹配的签名描述文件。\n\
         建议：\n\
         1) 确认 iPhone 已开启“开发者模式”（设置 → 隐私与安全性 → 开发者模式）并重启；\n\
         2) 在 AutoSign 设置里重新登录 Apple ID 后重试；\n\
         3) 该 App 若带扩展或多个 Bundle（例如 LiveContainer+SideStore），免费账号可能不被支持，需要用付费开发者账号。"
    } else if error.contains("0xe8008018") || error.contains("no longer valid") {
        "含义：签名证书已失效或被 Apple 吊销（电脑时间不准也会触发）。\n\
         建议：\n\
         1) 先校准电脑与手机的日期时间，保持同一时区；\n\
         2) 在 AutoSign 设置里重置证书（会吊销旧的），再重新签名安装。"
    } else if error.contains("0xe8008014") || error.contains("0xe8008013") {
        "含义：设备未开启开发者模式，或未信任这台电脑。\n\
         建议：iPhone 上开启“开发者模式”并重启；插线后点“信任此电脑”。"
    } else if error.contains("0xe8008016") {
        "含义：该 App 使用的权限（entitlements）免费账号不支持。\n\
         建议：这类 App 需要付费开发者账号签名。"
    } else if error.contains("0xe8008012") {
        "含义：这台设备没有登记到 Apple 开发者账号里（免费账号最多 3 台设备）。\n\
         建议：在 Apple 开发者后台或 Xcode 中移除不用的设备后重试。"
    } else if error.contains("0xe8008001") {
        "含义：签名本身无效（通常是 IPA 没有被正确重签）。\n\
         建议：重新导入这个 IPA 再安装一次。"
    } else {
        return None;
    };

    Some(hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_unknown_errors_through() {
        let error = "Something unrelated went wrong";
        assert_eq!(enrich_install_error(error), error);
    }

    #[test]
    fn explains_missing_profile_and_keeps_original_text() {
        let error = "Install error: 0xe8008015 (A valid provisioning profile for this executable was not found.)";
        let enriched = enrich_install_error(error);

        assert!(enriched.starts_with(error), "{enriched}");
        assert!(enriched.contains("开发者模式"), "{enriched}");
        assert!(enriched.contains("LiveContainer"), "{enriched}");
    }

    #[test]
    fn explains_revoked_certificate() {
        let enriched = enrich_install_error("Failed to verify code signature: 0xe8008018");
        assert!(enriched.contains("吊销"), "{enriched}");
        assert!(enriched.contains("重置证书"), "{enriched}");
    }

    #[test]
    fn explains_developer_mode_requirement() {
        let enriched = enrich_install_error("installd error 0xe8008014");
        assert!(enriched.contains("信任"), "{enriched}");
    }
}
