mod login;
mod token;
mod two_factor_auth;

use aes::cipher::BlockModeDecrypt;
use cbc::cipher::{KeyIvInit, block_padding::Pkcs7};
use hmac::{Hmac, KeyInit, Mac};
use reqwest::{Client, Response, header::HeaderMap};
use sha2::Sha256;
use srp::ClientVerifier;
use std::{io::Cursor, time::Duration};

use crate::Error;

pub async fn parse_response(
    res: Result<Response, reqwest::Error>,
    stage: &str,
) -> Result<plist::Dictionary, Error> {
    let mut root = parse_dictionary_response(res, stage).await?;
    match root.remove("Response") {
        Some(plist::Value::Dictionary(dict)) => Ok(dict),
        _ => Err(unexpected_auth_response(
            stage,
            None,
            "valid plist without a Response dictionary",
        )),
    }
}

pub async fn parse_dictionary_response(
    res: Result<Response, reqwest::Error>,
    stage: &str,
) -> Result<plist::Dictionary, Error> {
    let response = res?;
    let http_code = Some(response.status().as_u16());
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown")
        .to_string();
    let body = response.bytes().await?;

    parse_dictionary_body(stage, http_code, &content_type, &body)
}

fn parse_dictionary_body(
    stage: &str,
    http_code: Option<u16>,
    content_type: &str,
    body: &[u8],
) -> Result<plist::Dictionary, Error> {
    let trimmed = body
        .iter()
        .copied()
        .skip_while(|byte| byte.is_ascii_whitespace())
        .collect::<Vec<_>>();
    let lower_prefix =
        String::from_utf8_lossy(&trimmed[..trimmed.len().min(32)]).to_ascii_lowercase();

    if lower_prefix.starts_with("<html") || lower_prefix.starts_with("<!doctype html") {
        log_unexpected_response(stage, http_code, &content_type, &body);
        return Err(unexpected_auth_response(stage, http_code, &content_type));
    }

    let value = plist::Value::from_reader(Cursor::new(&body)).map_err(|_| {
        log_unexpected_response(stage, http_code, &content_type, &body);
        unexpected_auth_response(stage, http_code, &content_type)
    })?;

    value.into_dictionary().ok_or_else(|| {
        log_unexpected_response(stage, http_code, &content_type, &body);
        unexpected_auth_response(stage, http_code, &content_type)
    })
}

pub async fn post_plist_with_retry(
    client: &Client,
    url: &str,
    headers: &HeaderMap,
    body: &[u8],
    stage: &str,
) -> Result<plist::Dictionary, Error> {
    const MAX_ATTEMPTS: u32 = 3;

    for attempt in 1..=MAX_ATTEMPTS {
        let response = client
            .post(url)
            .headers(headers.clone())
            .body(body.to_vec())
            .send()
            .await;

        match parse_response(response, stage).await {
            Ok(dictionary) => return Ok(dictionary),
            Err(error) if is_retryable_auth_error(&error) && attempt < MAX_ATTEMPTS => {
                log::warn!(
                    "Apple sign-in response failed during {stage}; retrying ({attempt}/{MAX_ATTEMPTS}): {error}"
                );
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Err(error) => return Err(error),
        }
    }

    unreachable!("the authentication retry loop always returns")
}

pub fn is_retryable_auth_error(error: &Error) -> bool {
    matches!(
        error,
        Error::UnexpectedAuthResponse { .. } | Error::Reqwest(_)
    )
}

fn unexpected_auth_response(stage: &str, http_code: Option<u16>, content_type: &str) -> Error {
    Error::UnexpectedAuthResponse {
        stage: stage.to_string(),
        http_code,
        content_type: content_type.to_string(),
    }
}

fn log_unexpected_response(stage: &str, http_code: Option<u16>, content_type: &str, body: &[u8]) {
    log::warn!(
        "Unexpected Apple sign-in response during {stage}: HTTP {http_code:?}, content type {content_type}, {} byte(s); response body omitted",
        body.len()
    );
}

pub fn check_error(res: &plist::Dictionary) -> Result<(), Error> {
    let res = match res.get("Status") {
        Some(plist::Value::Dictionary(d)) => d,
        _ => &res,
    };

    if res.get("ec").unwrap().as_signed_integer().unwrap() != 0 {
        return Err(Error::AuthSrpWithMessage(
            res.get("ec").unwrap().as_signed_integer().unwrap().into(),
            res.get("em").unwrap().as_string().unwrap().to_owned(),
        ));
    }

    Ok(())
}

pub fn decrypt_cbc(usr: &ClientVerifier<Sha256>, data: &[u8]) -> Vec<u8> {
    let extra_data_key = create_session_key(usr, "extra data key:");
    let extra_data_iv = create_session_key(usr, "extra data iv:");
    let extra_data_iv = &extra_data_iv[..16];

    cbc::Decryptor::<aes::Aes256>::new_from_slices(&extra_data_key, extra_data_iv)
        .unwrap()
        .decrypt_padded_vec::<Pkcs7>(&data)
        .unwrap()
}

pub fn create_session_key(usr: &ClientVerifier<Sha256>, name: &str) -> Vec<u8> {
    Hmac::<Sha256>::new_from_slice(&usr.key())
        .unwrap()
        .chain_update(name.as_bytes())
        .finalize()
        .into_bytes()
        .to_vec()
}

#[cfg(test)]
mod tests {
    use super::{parse_dictionary_body, unexpected_auth_response};
    use crate::Error;

    #[test]
    fn unexpected_auth_response_hides_html_body() {
        let error = unexpected_auth_response("initial login", Some(200), "text/html");
        let message = error.to_string();

        assert!(matches!(error, Error::UnexpectedAuthResponse { .. }));
        assert!(message.contains("initial login"));
        assert!(message.contains("text/html"));
        assert!(!message.contains("<html"));
    }

    #[test]
    fn html_auth_response_is_classified_without_a_plist_error() {
        let error = parse_dictionary_body(
            "initial login",
            Some(200),
            "text/html",
            b"<html><body>temporary upstream page</body></html>",
        )
        .unwrap_err();

        assert!(matches!(error, Error::UnexpectedAuthResponse { .. }));
        assert!(!error.to_string().contains("temporary upstream page"));
    }

    #[test]
    fn plist_dictionary_response_is_accepted() {
        let mut expected = plist::Dictionary::new();
        expected.insert("result".to_string(), plist::Value::String("ok".to_string()));
        let mut body = Vec::new();
        plist::to_writer_xml(&mut body, &expected).unwrap();

        let parsed =
            parse_dictionary_body("initial login", Some(200), "text/x-xml-plist", &body).unwrap();

        assert_eq!(parsed, expected);
    }
}
