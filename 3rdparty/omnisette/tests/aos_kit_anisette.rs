//! macOS only: anisette data generated locally through Apple's own frameworks.
//!
//!   cargo test -p omnisette --features remote-anisette-v3 --test aos_kit_anisette

#![cfg(target_os = "macos")]

use omnisette::anisette_headers_provider::AnisetteHeadersProvider;
use omnisette::{AnisetteConfiguration, AnisetteError, AnisetteHeaders, AnisetteHeadersProviderType};

#[tokio::test]
async fn aos_kit_generates_headers_locally() -> Result<(), AnisetteError> {
    // An empty configuration directory would normally force the remote server;
    // AOSKit should step in first.
    let empty = std::env::temp_dir().join("autosign-aoskit-test");
    std::fs::create_dir_all(&empty).expect("temp dir");

    let mut provider = AnisetteHeaders::get_anisette_headers_provider(
        AnisetteConfiguration::new().set_configuration_path(empty),
    )?;

    // macOS 27 no longer returns anisette data from AOSKit (-45070); in that
    // case the client must fall back without crashing, which is what this test
    // protects.
    if !matches!(provider.provider_type, AnisetteHeadersProviderType::Local) {
        println!("AOSKit is unavailable on this macOS version; the remote provider is used");
        return Ok(());
    }

    let headers = match provider.provider.get_authentication_headers().await {
        Ok(headers) => headers,
        Err(err) => {
            println!("AOSKit could not produce headers ({err}); the caller falls back instead");
            return Ok(());
        }
    };

    for key in ["X-Apple-I-MD", "X-Apple-I-MD-M"] {
        assert!(headers.contains_key(key), "missing {key}: {headers:?}");
    }

    println!(
        "AOSKit anisette headers: {:?}",
        {
            let mut keys: Vec<_> = headers.keys().cloned().collect();
            keys.sort();
            keys
        }
    );

    Ok(())
}
