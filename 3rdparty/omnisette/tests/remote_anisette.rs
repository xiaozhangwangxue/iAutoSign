//! Exercises the remote (v3) anisette server.
//!
//! This is the path the macOS build (and any machine without the bundled Apple
//! libraries) uses when signing in. Requires internet access.
//!
//! Run with:
//!   cargo test -p omnisette --features remote-anisette-v3 --test remote_anisette -- --ignored --nocapture

use std::path::PathBuf;

use omnisette::anisette_headers_provider::AnisetteHeadersProvider;
use omnisette::{
    AnisetteConfiguration, AnisetteError, AnisetteHeaders, AnisetteHeadersProviderType,
};

#[tokio::test]
#[ignore = "requires internet access"]
async fn remote_provider_generates_headers() -> Result<(), AnisetteError> {
    // An empty directory makes the local provider report "missing libraries",
    // so the remote one is picked - exactly what happens on macOS.
    let empty: PathBuf = std::env::temp_dir().join("autosign-remote-anisette-test");
    std::fs::create_dir_all(&empty).expect("temp dir");

    let configuration =
        AnisetteConfiguration::new().set_configuration_path(empty.clone());

    let mut provider = AnisetteHeaders::get_anisette_headers_provider(configuration)?;

    assert!(
        matches!(provider.provider_type, AnisetteHeadersProviderType::Remote),
        "the remote anisette provider should be used when the local libraries are missing"
    );

    let headers = provider.provider.get_authentication_headers().await?;

    for key in ["X-Apple-I-MD", "X-Apple-I-MD-M"] {
        assert!(headers.contains_key(key), "missing {key}: {headers:?}");
    }

    println!(
        "remote anisette headers generated: {:?}",
        {
            let mut keys: Vec<_> = headers.keys().cloned().collect();
            keys.sort();
            keys
        }
    );

    Ok(())
}
