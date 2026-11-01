// Bodian's desktop API as bodian-music-api 0.1.2 speaks it: the device headers, the signed query and body, code 200.

use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::OmniError;

const API_ORIGIN: &str = "https://bd-api.kuwo.cn";
const LYRIC_ORIGIN: &str = "https://mlyric.kuwo.cn";
/// The session code that means the sign-in is gone.
const SIGNED_OUT: i64 = 11012;

/// A stable device id and, once signed in, the account (`sessions.set({ uid, token, user })`). Never logged.
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub device_id: String,
    #[serde(default)]
    pub uid: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

impl Session {
    pub fn signed_in(&self) -> bool {
        self.uid.is_some() && self.token.as_deref().is_some_and(|token| !token.is_empty())
    }
}

/// One `client.call`.
#[derive(Default)]
pub struct Call<'a> {
    pub params: Vec<(&'a str, String)>,
    pub body: Option<Value>,
    pub post: bool,
    pub signed: bool,
    pub anonymous: bool,
    pub lyric: bool,
}

pub struct Client {
    http: reqwest::Client,
    session: Mutex<Session>,
}

impl Client {
    pub fn new(http: reqwest::Client) -> Self {
        let session = Session { device_id: device_id(), uid: None, token: None, nickname: None, avatar_url: None };
        Self { http, session: Mutex::new(session) }
    }

    pub fn session(&self) -> Session {
        self.session.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn set_session(&self, session: Session) {
        *self.session.lock().unwrap_or_else(PoisonError::into_inner) = session;
    }

    /// `createBodianClient().call`: `uid` and `token` on every query, a signature over the query and the body.
    pub async fn call(&self, path: &str, call: Call<'_>) -> Result<Value, OmniError> {
        let session = self.session();
        let signed_in = !call.anonymous && session.signed_in();
        let mut query: Vec<(String, String)> = call.params.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect();
        let uid = if signed_in { session.uid.clone().unwrap_or_default() } else { "-1".to_owned() };
        let token = if signed_in { session.token.clone().unwrap_or_default() } else { String::new() };
        query.push(("uid".to_owned(), uid.clone()));
        query.push(("token".to_owned(), token.clone()));
        let body = call.body.as_ref().map(Value::to_string).unwrap_or_default();
        if call.signed {
            let millis = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
            query.push(("timestamp".to_owned(), millis.to_string()));
            let sign = sign_query(path, &query, &body);
            query.push(("sign".to_owned(), sign));
        }
        let origin = if call.lyric { LYRIC_ORIGIN } else { API_ORIGIN };
        let url = reqwest::Url::parse_with_params(&format!("{origin}{path}"), &query)
            .map_err(|err| OmniError::Response(format!("bodian address: {err}")))?;
        let mut request = if call.post { self.http.post(url) } else { self.http.get(url) };
        for (name, value) in [
            ("User-Agent", "Dart/3.3 (dart:io)"),
            ("plat", "win"),
            ("channel", "W1"),
            ("ver", "1.1.7"),
            ("svrver", "13"),
            ("api-ver", "application/json"),
            ("brand", "Windows"),
            ("net", "wifi"),
            ("Content-Type", "application/json"),
        ] {
            request = request.header(name, value);
        }
        request = request.header("devid", &session.device_id).header("qimei36", &session.device_id);
        if signed_in {
            request = request.header("uid", uid).header("token", token);
        }
        if !body.is_empty() {
            request = request.body(body);
        }
        let response = request.send().await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(OmniError::SignedOut);
        }
        let answer: Value = response.error_for_status()?.json().await?;
        match answer.get("code").and_then(|code| code.as_i64().or_else(|| code.as_str()?.parse().ok())) {
            Some(200) => Ok(answer),
            Some(SIGNED_OUT) => Err(OmniError::SignedOut),
            Some(20018) => Err(OmniError::Unavailable),
            Some(code) => Err(OmniError::Platform(code)),
            None => Err(OmniError::Response("bodian answer without a code".to_owned())),
        }
    }
}

/// `signQuery`: the query as `URLSearchParams` writes it, its letters and digits sorted, the body's MD5, the path.
fn sign_query(path: &str, query: &[(String, String)], body: &str) -> String {
    let encoded =
        query.iter().map(|(k, v)| format!("{}={}", form_encode(k), form_encode(v))).collect::<Vec<_>>().join("&");
    let mut chars: Vec<char> = encoded.chars().filter(char::is_ascii_alphanumeric).collect();
    chars.sort_unstable();
    let mut seed = format!("kuwotest{}", chars.into_iter().collect::<String>());
    if !body.is_empty() {
        seed.push_str(&md5_hex(&format!("{body}kuwotest")));
    }
    md5_hex(&format!("{seed}{path}"))
}

/// `application/x-www-form-urlencoded` as `URLSearchParams` serialises it.
fn form_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => char::from(byte).to_string(),
            b' ' => "+".to_owned(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn md5_hex(text: &str) -> String {
    Md5::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `crypto.randomBytes(16).toString('hex')`, from the standard library's random hasher keys and the clock.
fn device_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::with_capacity(32);
    for round in 0..2u64 {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(round);
        hasher.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos());
        out.push_str(&format!("{:016x}", hasher.finish()));
    }
    out
}
