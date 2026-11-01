// Spotify sign-in as `signIn.cjs` does it: PKCE in the system browser, the answer caught on 127.0.0.1:5588.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::Value;

use super::api::Tokens;
use crate::OmniError;

pub const CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
const PORT: u16 = 5588;
const REDIRECT_URI: &str = "http://127.0.0.1:5588/login";
const AUTHORIZE_URL: &str = "https://accounts.spotify.com/authorize";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const ATTEMPT: Duration = Duration::from_secs(10 * 60);
const SCOPES: &str = "app-remote-control playlist-modify playlist-modify-private playlist-modify-public playlist-read \
playlist-read-collaborative playlist-read-private streaming ugc-image-upload user-follow-modify user-follow-read \
user-library-modify user-library-read user-modify user-modify-playback-state user-modify-private user-personalized \
user-read-birthdate user-read-currently-playing user-read-email user-read-play-history user-read-playback-position \
user-read-playback-state user-read-private user-read-recently-played user-top-read";
const DONE_PAGE: &str = "已登录 Spotify，可以回到 Folia 了。";
const FAILED_PAGE: &str = "登录未完成。请回到 Folia 重试。";

/// What the browser sent back.
#[derive(Clone)]
pub enum Outcome {
    Code(String),
    Error(String),
}

/// One sign-in in progress: its key, PKCE verifier, the page to open, and what the listener caught.
pub struct Attempt {
    pub key: String,
    pub verifier: String,
    pub page: String,
    pub outcome: Arc<Mutex<Option<Outcome>>>,
    pub stop: Arc<Mutex<bool>>,
    pub until: Instant,
}

impl Drop for Attempt {
    fn drop(&mut self) {
        *self.stop.lock().unwrap_or_else(PoisonError::into_inner) = true;
    }
}

/// `start`: listens on the loopback port in a background thread and builds the authorize page.
pub fn start(rng: &SystemRandom) -> Result<Attempt, OmniError> {
    let verifier = URL_SAFE_NO_PAD.encode(random(rng, 64));
    let state = URL_SAFE_NO_PAD.encode(random(rng, 16));
    let key: String = random(rng, 16).iter().map(|b| format!("{b:02x}")).collect();
    let challenge = URL_SAFE_NO_PAD.encode(ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes()));
    let page = reqwest::Url::parse_with_params(
        AUTHORIZE_URL,
        [
            ("response_type", "code"),
            ("client_id", CLIENT_ID),
            ("redirect_uri", REDIRECT_URI),
            ("scope", SCOPES),
            ("code_challenge_method", "S256"),
            ("code_challenge", challenge.as_str()),
            ("state", state.as_str()),
        ],
    )
    .map_err(|err| OmniError::Response(err.to_string()))?
    .to_string();
    let listener = TcpListener::bind(("127.0.0.1", PORT))
        .map_err(|source| OmniError::Io { path: std::path::PathBuf::from(REDIRECT_URI), source })?;
    listener
        .set_nonblocking(true)
        .map_err(|source| OmniError::Io { path: std::path::PathBuf::from(REDIRECT_URI), source })?;
    let outcome = Arc::new(Mutex::new(None));
    let stop = Arc::new(Mutex::new(false));
    let until = Instant::now() + ATTEMPT;
    let (caught, stopped) = (outcome.clone(), stop.clone());
    std::thread::spawn(move || listen(listener, &state, &caught, &stopped, until));
    tracing::info!("spotify sign-in is waiting on 127.0.0.1:{PORT}");
    Ok(Attempt { key, verifier, page, outcome, stop, until })
}

/// Answers `GET /login?code=…&state=…` once; anything else is a 404. Ends with the attempt.
fn listen(listener: TcpListener, state: &str, caught: &Mutex<Option<Outcome>>, stop: &Mutex<bool>, until: Instant) {
    while Instant::now() < until && !*stop.lock().unwrap_or_else(PoisonError::into_inner) {
        let Ok((mut stream, _)) = listener.accept() else {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buffer = [0u8; 8192];
        let read = stream.read(&mut buffer).unwrap_or(0);
        let request = String::from_utf8_lossy(&buffer[..read]);
        let target =
            request.lines().next().and_then(|line| line.strip_prefix("GET ")).and_then(|rest| rest.split(' ').next());
        let Some(url) = target.and_then(|path| reqwest::Url::parse(&format!("http://127.0.0.1:{PORT}{path}")).ok())
        else {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            continue;
        };
        let param = |name: &str| url.query_pairs().find(|(key, _)| key == name).map(|(_, value)| value.into_owned());
        if url.path() != "/login" {
            let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            continue;
        }
        if param("state").as_deref() != Some(state) {
            respond(&mut stream, FAILED_PAGE);
            continue;
        }
        let outcome = match (param("code"), param("error")) {
            (Some(code), _) => Outcome::Code(code),
            (None, Some(error)) => Outcome::Error(error),
            (None, None) => Outcome::Error("sign-in-failed".to_owned()),
        };
        respond(&mut stream, if matches!(outcome, Outcome::Code(_)) { DONE_PAGE } else { FAILED_PAGE });
        *caught.lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome);
        return;
    }
}

fn respond(stream: &mut std::net::TcpStream, words: &str) {
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Folia</title><body style=\"margin:0;display:grid;place-items:center;height:100vh;background:#121212;color:#fff;font:16px system-ui,sans-serif\">{words}</body>"
    );
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

/// `tokens`: the token endpoint for a code (`authorization_code`) or a renewal (`refresh_token`).
pub async fn tokens(http: &reqwest::Client, fields: &[(&str, &str)], kept: Option<&str>) -> Result<Tokens, OmniError> {
    let mut form: Vec<(&str, &str)> = vec![("client_id", CLIENT_ID)];
    form.extend_from_slice(fields);
    let response = http.post(TOKEN_URL).form(&form).send().await?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if body.get("error").and_then(Value::as_str) == Some("invalid_grant") {
        return Err(OmniError::SignedOut);
    }
    let access = body.get("access_token").and_then(Value::as_str);
    let refresh = body.get("refresh_token").and_then(Value::as_str).or(kept);
    match (status.is_success(), access, refresh) {
        (true, Some(access), Some(refresh)) => {
            let lifetime = body.get("expires_in").and_then(Value::as_u64).unwrap_or(3600).max(60);
            Ok(Tokens {
                access_token: access.to_owned(),
                refresh_token: refresh.to_owned(),
                expires_at: super::api::now_millis() + lifetime * 1000,
            })
        }
        _ => Err(OmniError::Response(format!("Spotify did not issue a token (HTTP {})", status.as_u16()))),
    }
}

/// Exchanges a caught code with the attempt's verifier.
pub async fn exchange(http: &reqwest::Client, code: &str, verifier: &str) -> Result<Tokens, OmniError> {
    tokens(
        http,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", verifier),
        ],
        None,
    )
    .await
}

fn random(rng: &SystemRandom, len: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; len];
    let _ = rng.fill(&mut bytes);
    bytes
}
