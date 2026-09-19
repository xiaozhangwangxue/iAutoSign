//! Verifies that anisette data can be generated **locally** from the bundled
//! Apple Music / ADI Android libraries - the same mechanism AutoSign relies on
//! so that signing does not depend on a third party anisette server.
//!
//! Requires the Android libraries for the host architecture to be present at
//! `anisette_test/lib/<arch>/{libstoreservicescore,libCoreADI}.so`.
//!
//! Run with: `cargo test -p omnisette --test local_anisette -- --nocapture`
//!
//! Both tests are ignored by default because they need the (non-redistributable)
//! Apple Music libraries; run them explicitly with `-- --ignored`.

use std::path::PathBuf;

use omnisette::anisette_headers_provider::AnisetteHeadersProvider;
use omnisette::{
    AnisetteConfiguration, AnisetteError, AnisetteHeaders, AnisetteHeadersProviderType,
};

#[tokio::test]
#[ignore = "requires the Apple Music Android libraries to be present"]
async fn local_provider_generates_headers() -> Result<(), AnisetteError> {
    let configuration =
        AnisetteConfiguration::new().set_configuration_path(PathBuf::from("anisette_test"));

    let mut provider = AnisetteHeaders::get_anisette_headers_provider(configuration)?;

    assert!(
        matches!(provider.provider_type, AnisetteHeadersProviderType::Local),
        "the local anisette provider should be used when the libraries are present"
    );

    let headers = provider.provider.get_authentication_headers().await?;

    for key in [
        "X-Apple-I-MD",
        "X-Apple-I-MD-M",
        "X-Apple-I-MD-RINFO",
        "X-Apple-I-MD-LU",
        "X-Apple-I-SRL-NO",
    ] {
        assert!(
            headers.contains_key(key),
            "missing header {key}: {headers:?}"
        );
    }

    println!(
        "local anisette headers generated: {:?}",
        {
            let mut keys: Vec<_> = headers.keys().cloned().collect();
            keys.sort();
            keys
        }
    );

    Ok(())
}

/// Only checks that the bundled Android libraries can be loaded and prepared
/// (no network access), which is the part that fails on machines where the
/// libraries are missing or incompatible.
#[test]
#[ignore = "requires the Apple Music Android libraries to be present"]
fn local_libraries_load() -> Result<(), AnisetteError> {
    let configuration =
        AnisetteConfiguration::new().set_configuration_path(PathBuf::from("anisette_test"));

    let provider = AnisetteHeaders::get_ssc_anisette_headers_provider(configuration)?;

    assert!(
        matches!(provider.provider_type, AnisetteHeadersProviderType::Local),
        "the bundled libraries should provide a local anisette provider"
    );

    Ok(())
}
