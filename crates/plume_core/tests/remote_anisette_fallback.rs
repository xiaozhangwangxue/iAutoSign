//! The public anisette servers come and go; when the configured one does not
//! answer, signing in has to fall back to another official server instead of
//! failing.
//!
//! Requires internet access; run with:
//!   cargo test -p plume_core --test remote_anisette_fallback -- --ignored --nocapture

use plume_core::{AnisetteConfiguration, auth::Account};

#[tokio::test]
#[ignore = "requires internet access"]
async fn falls_back_to_another_server() {
    // The application installs this at start-up; the test has to do it too.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Port 9 is the discard port: nothing listens there, so the first attempt
    // always fails and the fallback list has to save us.
    let configuration = AnisetteConfiguration::new()
        .set_configuration_path(std::env::temp_dir().join("autosign-anisette-fallback"))
        .set_anisette_url_v3("https://127.0.0.1:9".to_string());

    // `Account::new` is what the login screen calls: it has to fetch anisette
    // data before it can talk to Apple, so succeeding here means a fallback
    // server answered.
    let account = Account::new(configuration)
        .await
        .expect("another anisette server should answer");

    println!("fallback anisette provisioning succeeded: {account:?}");
}
