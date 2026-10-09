use std::collections::HashMap;
use std::time::SystemTime;

use omnisette::{AnisetteConfiguration, AnisetteHeaders};

use crate::Error;

#[derive(Debug, Clone)]
pub struct AnisetteData {
    pub base_headers: HashMap<String, String>,
    pub generated_at: SystemTime,
    pub config: AnisetteConfiguration,
}

impl AnisetteData {
    pub async fn new(config: AnisetteConfiguration) -> Result<Self, Error> {
        // The public anisette servers are shared and occasionally time out, so
        // walk through the officially recommended ones until one answers. The
        // configured server is always tried first.
        const FALLBACK_SERVERS: &[&str] = &[
            "https://ani.sidestore.io",
            "https://ani.sidestore.zip",
            "https://ani.npeg.us",
            "https://anisette.wedotstud.io",
            "https://ani.xu30.top",
            "https://ani.owoellen.rocks",
        ];

        let mut candidates = vec![config.anisette_url_v3().clone()];
        for server in FALLBACK_SERVERS {
            if !candidates.iter().any(|candidate| candidate == server) {
                candidates.push(server.to_string());
            }
        }

        let mut last_error: Option<omnisette::AnisetteError> = None;

        for (index, server) in candidates.iter().enumerate() {
            let candidate_config = config.clone().set_anisette_url_v3(server.clone());

            // The first attempt may use a local source (Apple's frameworks on
            // macOS, the bundled libraries elsewhere); every later attempt goes
            // straight to a remote server.
            let mut provider = if index == 0 {
                match AnisetteHeaders::get_anisette_headers_provider(candidate_config.clone()) {
                    Ok(provider) => provider,
                    Err(err) => {
                        last_error = Some(err);
                        continue;
                    }
                }
            } else {
                AnisetteHeaders::get_remote_anisette_headers_provider(candidate_config.clone())
            };

            match provider.provider.get_authentication_headers().await {
                Ok(base_headers) => {
                    log::info!("Anisette data obtained from {server}");

                    return Ok(AnisetteData {
                        base_headers,
                        generated_at: SystemTime::now(),
                        config: candidate_config,
                    });
                }
                Err(err) => {
                    log::warn!(
                        "Anisette server {server} failed ({}/{}): {err}",
                        index + 1,
                        candidates.len()
                    );
                    last_error = Some(err);
                }
            }
        }

        Err(Error::Anisette(last_error.unwrap_or(
            omnisette::AnisetteError::MissingLibraries,
        )))
    }

    pub fn needs_refresh(&self) -> bool {
        let elapsed = self.generated_at.elapsed().unwrap();
        elapsed.as_secs() > 60
    }

    pub fn is_valid(&self) -> bool {
        let elapsed = self.generated_at.elapsed().unwrap();
        elapsed.as_secs() < 90
    }

    pub async fn refresh(&self) -> Result<Self, crate::Error> {
        Self::new(self.config.clone()).await
    }

    pub fn generate_headers(
        &self,
        cpd: bool,
        client_info: bool,
        app_info: bool,
    ) -> HashMap<String, String> {
        if !self.is_valid() {
            panic!("Invalid data!")
        }

        let mut headers = self.base_headers.clone();
        let old_client_info = headers.remove("X-Mme-Client-Info");

        if client_info {
            let client_info = match old_client_info {
                Some(v) => {
                    let temp = v.as_str();

                    temp.replace(
                        temp.split('<').nth(3).unwrap().split('>').nth(0).unwrap(),
                        "com.apple.AuthKit/1 (com.apple.akd/1.0)",
                    )
                }
                None => {
                    return headers;
                }
            };
            headers.insert("X-Mme-Client-Info".to_owned(), client_info.to_owned());
        }

        if app_info {
            headers.insert(
                "X-Apple-App-Info".to_owned(),
                "com.apple.gs.xcode.auth".to_owned(),
            );
            headers.insert("X-Xcode-Version".to_owned(), "11.2 (11B41)".to_owned());
        }

        if cpd {
            headers.insert("bootstrap".to_owned(), "true".to_owned());
            headers.insert("icscrec".to_owned(), "true".to_owned());
            headers.insert("loc".to_owned(), "en_GB".to_owned());
            headers.insert("pbe".to_owned(), "false".to_owned());
            headers.insert("prkgen".to_owned(), "true".to_owned());
            headers.insert("svct".to_owned(), "iCloud".to_owned());
        }

        headers
    }

    pub fn to_plist(&self, cpd: bool, client_info: bool, app_info: bool) -> plist::Dictionary {
        let mut plist = plist::Dictionary::new();
        for (key, value) in self.generate_headers(cpd, client_info, app_info).iter() {
            plist.insert(key.to_owned(), plist::Value::String(value.to_owned()));
        }

        plist
    }

    pub fn get_header(&self, header: &str) -> Result<String, Error> {
        let headers = self
            .generate_headers(true, true, true)
            .iter()
            .map(|(k, v)| (k.to_lowercase(), v.to_lowercase()))
            .collect::<HashMap<String, String>>();

        match headers.get(&header.to_lowercase()) {
            Some(v) => Ok(v.to_string()),
            None => Err(Error::DeveloperSessionRequestFailed),
        }
    }
}
