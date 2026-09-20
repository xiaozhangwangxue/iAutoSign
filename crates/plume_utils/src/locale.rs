//! Maps the operating system's locale name onto the locale files that ship with
//! the application.
//!
//! Windows reports names like `zh-CN` or `zh-Hans-CN` while the translation
//! files are named `zh_cn.toml` / `zh_tw.toml`. Passing the raw name through
//! would not match anything and the interface would silently fall back to
//! English, which is exactly what used to happen on Chinese Windows.

/// Locale used when the system locale is not one we ship.
pub const FALLBACK_LOCALE: &str = "zh_cn";

/// Returns the locale code to use for `system_locale`.
///
/// `available` is the list of locale names the application ships.
pub fn normalize_locale<I, S>(system_locale: &str, available: I, fallback: &str) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let available: Vec<String> = available
        .into_iter()
        .map(|code| code.as_ref().to_string())
        .collect();

    let lower = system_locale.trim().to_lowercase().replace('-', "_");

    if lower.is_empty() {
        return fallback.to_string();
    }

    // Chinese needs explicit handling because simplified and traditional
    // variants both start with `zh`.
    if lower.starts_with("zh") {
        let traditional = ["tw", "hk", "mo", "hant"]
            .iter()
            .any(|marker| lower.contains(marker));

        let wanted = if traditional { "zh_tw" } else { "zh_cn" };

        if available.iter().any(|code| code == wanted) {
            return wanted.to_string();
        }
    }

    if let Some(exact) = available
        .iter()
        .find(|code| code.eq_ignore_ascii_case(&lower))
    {
        return exact.clone();
    }

    // Fall back to the language part alone (`pt_br` -> `pt`).
    if let Some(language) = lower.split('_').next() {
        if let Some(exact) = available
            .iter()
            .find(|code| code.eq_ignore_ascii_case(language))
        {
            return exact.clone();
        }
    }

    fallback.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const AVAILABLE: &[&str] = &["en", "zh_cn", "zh_tw", "ja", "de"];

    #[test]
    fn windows_chinese_names_map_to_our_files() {
        for name in ["zh-CN", "zh-Hans-CN", "zh_cn", "zh-Hans", "zh-SG"] {
            assert_eq!(normalize_locale(name, AVAILABLE, "en"), "zh_cn", "{name}");
        }
    }

    #[test]
    fn traditional_chinese_maps_to_taiwanese_file() {
        for name in ["zh-TW", "zh-Hant-TW", "zh-HK", "zh-MO"] {
            assert_eq!(normalize_locale(name, AVAILABLE, "en"), "zh_tw", "{name}");
        }
    }

    #[test]
    fn other_languages_are_matched_case_insensitively() {
        assert_eq!(normalize_locale("en-US", AVAILABLE, "zh_cn"), "en");
        assert_eq!(normalize_locale("JA", AVAILABLE, "zh_cn"), "ja");
        assert_eq!(normalize_locale("de-DE", AVAILABLE, "zh_cn"), "de");
    }

    #[test]
    fn unknown_languages_use_the_fallback() {
        assert_eq!(normalize_locale("pt-BR", AVAILABLE, "zh_cn"), "zh_cn");
        assert_eq!(normalize_locale("", AVAILABLE, "zh_cn"), "zh_cn");
    }
}
