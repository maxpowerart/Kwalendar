//! Google OAuth 2.0 for installed apps: loopback redirect + PKCE. Refresh tokens live in the OS keyring.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use rand::RngCore;
use reqwest::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::google::ApiError;

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub const USERINFO_URL: &str = "https://openidconnect.googleapis.com/v1/userinfo";
const SCOPES: &str = "openid email profile https://www.googleapis.com/auth/calendar";
const KEYRING_SERVICE: &str = "kwalendar";

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<i64>,
    refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UserInfo {
    pub sub: String,
    pub email: String,
    pub name: Option<String>,
}

pub struct LoggedIn {
    pub user: UserInfo,
    pub access_token: String,
    pub expires_at: i64,
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn random(n: usize) -> String {
    let mut buf = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut buf);
    b64(&buf)
}

pub fn store_refresh_token(account: &str, token: &str) -> Result<()> {
    keyring::Entry::new(KEYRING_SERVICE, account)?
        .set_password(token)
        .context("keyring write")
}

pub fn load_refresh_token(account: &str) -> Result<Option<String>> {
    match keyring::Entry::new(KEYRING_SERVICE, account)?.get_password() {
        Ok(t) => Ok(Some(t)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn delete_refresh_token(account: &str) {
    if let Ok(e) = keyring::Entry::new(KEYRING_SERVICE, account) {
        let _ = e.delete_credential();
    }
}

/// Runs the full interactive sign-in. `open_url` must open the system browser.
pub async fn login(
    http: &Client,
    token_url: &str,
    userinfo_url: &str,
    client_id: &str,
    client_secret: &str,
    login_hint: Option<&str>,
    open_url: impl FnOnce(String),
) -> Result<LoggedIn> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let redirect = format!("http://127.0.0.1:{port}");
    let verifier = random(48);
    let challenge = b64(&Sha256::digest(verifier.as_bytes()));
    let state = random(16);

    let mut url = format!(
        "{AUTH_URL}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256&state={}&access_type=offline&prompt=consent",
        urlencoding::encode(client_id),
        urlencoding::encode(&redirect),
        urlencoding::encode(SCOPES),
        challenge,
        state
    );
    if let Some(h) = login_hint {
        url.push_str(&format!("&login_hint={}", urlencoding::encode(h)));
    }
    open_url(url);

    let code = tokio::time::timeout(Duration::from_secs(300), wait_for_code(&listener, &state))
        .await
        .map_err(|_| anyhow!("Время входа истекло"))??;

    let resp = http
        .post(token_url)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code.as_str()),
            ("code_verifier", verifier.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect.as_str()),
        ])
        .send()
        .await?;
    if !resp.status().is_success() {
        bail!("Google отклонил вход: {}", resp.text().await.unwrap_or_default());
    }
    let tok: TokenResponse = resp.json().await?;
    let user: UserInfo = http
        .get(userinfo_url)
        .bearer_auth(&tok.access_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let refresh = tok
        .refresh_token
        .ok_or_else(|| anyhow!("Google не вернул refresh-токен"))?;
    store_refresh_token(&user.sub, &refresh)?;
    Ok(LoggedIn {
        user,
        access_token: tok.access_token,
        expires_at: chrono::Utc::now().timestamp() + tok.expires_in.unwrap_or(3600),
    })
}

async fn wait_for_code(listener: &TcpListener, state: &str) -> Result<String> {
    loop {
        let (mut sock, _) = listener.accept().await?;
        let mut buf = vec![0u8; 8192];
        let n = sock.read(&mut buf).await.unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let path = req.split_whitespace().nth(1).unwrap_or("/");
        let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
        let mut code = None;
        let mut got_state = None;
        let mut error = None;
        for pair in query.split('&') {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let v = urlencoding::decode(v).map(|c| c.into_owned()).unwrap_or_default();
            match k {
                "code" => code = Some(v),
                "state" => got_state = Some(v),
                "error" => error = Some(v),
                _ => {}
            }
        }
        if code.is_none() && error.is_none() {
            // favicon or a stray request
            let _ = sock.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await;
            continue;
        }
        let ok = error.is_none() && got_state.as_deref() == Some(state);
        let body = format!(
            "<!doctype html><meta charset=utf-8><title>Kwalendar</title><body style=\"font:16px system-ui;background:#0F1013;color:#ECEDEF;display:grid;place-items:center;height:100vh;margin:0\"><div>{}</div></body>",
            if ok { "Готово — вкладку можно закрыть и вернуться в Kwalendar." } else { "Вход отменён. Вернитесь в Kwalendar и попробуйте ещё раз." }
        );
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = sock.write_all(resp.as_bytes()).await;
        let _ = sock.shutdown().await;
        if let Some(e) = error {
            bail!("Вход отменён: {e}");
        }
        if got_state.as_deref() != Some(state) {
            bail!("Неверный state в ответе OAuth");
        }
        return Ok(code.unwrap());
    }
}

/// Exchanges the stored refresh token for a fresh access token.
pub async fn refresh(http: &Client, token_url: &str, client_id: &str, client_secret: &str, account: &str) -> Result<(String, i64), ApiError> {
    let refresh = load_refresh_token(account)
        .map_err(|e| ApiError::Other(e.to_string()))?
        .ok_or(ApiError::Revoked)?;
    let resp = http
        .post(token_url)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh.as_str()),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await?;
    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        if text.contains("invalid_grant") {
            return Err(ApiError::Revoked);
        }
        return Err(ApiError::Other(format!("token refresh: {text}")));
    }
    let tok: TokenResponse = resp.json().await?;
    if let Some(r) = tok.refresh_token {
        let _ = store_refresh_token(account, &r);
    }
    Ok((tok.access_token, chrono::Utc::now().timestamp() + tok.expires_in.unwrap_or(3600)))
}
