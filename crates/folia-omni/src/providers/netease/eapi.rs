// NetEase's client API (`eapi`): the session cookie fields, the request signature and its AES-128-ECB encryption.

use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use aes::Aes128;
use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockEncrypt, KeyInit};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::OmniError;

const DOMAIN: &str = "https://interface.music.163.com";
const KEY: &[u8; 16] = b"e82ckenh8dichen8";
const SEPARATOR: &str = "-36cd479b6b5-";
const APP_VERSION: &str = "3.1.17.204416";
const OS_VERSION: &str = "Microsoft-Windows-10-Professional-build-19045-64bit";
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/91.0.4472.164 NeteaseMusicDesktop/3.1.17.204416";

/// The cookie fields a signed-in request carries. Never logged.
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Session {
    pub device_id: String,
    #[serde(default)]
    pub music_u: Option<String>,
    #[serde(default)]
    pub csrf: Option<String>,
}

impl Session {
    pub fn guest() -> Self {
        Self { device_id: device_id(), music_u: None, csrf: None }
    }

    pub fn signed_in(&self) -> bool {
        self.music_u.as_deref().is_some_and(|value| !value.is_empty())
    }
}

/// An answer: its JSON and the `name=value` pairs of its `Set-Cookie` headers.
pub struct Answer {
    pub body: Value,
    pub cookies: Vec<(String, String)>,
}

pub struct Client {
    http: reqwest::Client,
    session: Mutex<Session>,
}

impl Client {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http, session: Mutex::new(Session::guest()) }
    }

    pub fn session(&self) -> Session {
        self.session.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn set_session(&self, session: Session) {
        *self.session.lock().unwrap_or_else(PoisonError::into_inner) = session;
    }

    /// Posts `params` to the client API at `path` (`/api/...`) with the session's cookie.
    pub async fn post(&self, path: &str, params: Map<String, Value>) -> Result<Answer, OmniError> {
        let session = self.session();
        let header = header_fields(&session);
        let mut data = params;
        data.insert(
            "header".to_owned(),
            Value::Object(header.iter().map(|(k, v)| (k.clone(), Value::from(v.as_str()))).collect()),
        );
        data.insert("e_r".to_owned(), Value::Bool(false));
        let text = Value::Object(data).to_string();
        let endpoint = path.strip_prefix("/api/").unwrap_or(path);
        let cookie = header.iter().map(|(k, v)| format!("{}={}", encode(k), encode(v))).collect::<Vec<_>>().join("; ");
        let response = self
            .http
            .post(format!("{DOMAIN}/eapi/{endpoint}"))
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header(reqwest::header::COOKIE, cookie)
            .header(reqwest::header::REFERER, "https://music.163.com")
            .form(&[("params", encrypt(path, &text))])
            .send()
            .await?
            .error_for_status()?;
        let cookies = response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.split(';').next())
            .filter_map(|pair| pair.split_once('='))
            .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
            .collect();
        let body = response.json().await?;
        Ok(Answer { body, cookies })
    }
}

fn header_fields(session: &Session) -> Vec<(String, String)> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let millis = now.as_millis();
    let mut fields = vec![
        ("osver", OS_VERSION.to_owned()),
        ("deviceId", session.device_id.clone()),
        ("os", "pc".to_owned()),
        ("appver", APP_VERSION.to_owned()),
        ("versioncode", "140".to_owned()),
        ("mobilename", String::new()),
        ("buildver", now.as_secs().to_string()),
        ("resolution", "1920x1080".to_owned()),
        ("__csrf", session.csrf.clone().unwrap_or_default()),
        ("channel", "netease".to_owned()),
        ("requestId", format!("{millis}_{:04}", millis % 1000)),
    ];
    if let Some(music_u) = session.music_u.as_ref().filter(|value| !value.is_empty()) {
        fields.push(("MUSIC_U", music_u.clone()));
    }
    fields.into_iter().map(|(name, value)| (name.to_owned(), value)).collect()
}

/// `params` of an eapi request: the path, the JSON and their MD5 signature, AES-128-ECB with PKCS#7, upper hex.
fn encrypt(path: &str, text: &str) -> String {
    let digest = hex(&Md5::digest(format!("nobody{path}use{text}md5forencrypt").as_bytes()), false);
    let mut message = format!("{path}{SEPARATOR}{text}{SEPARATOR}{digest}").into_bytes();
    let pad = 16 - message.len() % 16;
    message.extend(std::iter::repeat_n(pad as u8, pad));
    let cipher = Aes128::new(GenericArray::from_slice(KEY));
    for block in message.as_chunks_mut::<16>().0 {
        cipher.encrypt_block(GenericArray::from_mut_slice(&mut block[..]));
    }
    hex(&message, true)
}

fn hex(bytes: &[u8], upper: bool) -> String {
    let digits: &[u8; 16] = if upper { b"0123456789ABCDEF" } else { b"0123456789abcdef" };
    bytes
        .iter()
        .flat_map(|byte| [digits[usize::from(byte >> 4)], digits[usize::from(byte & 15)]])
        .map(char::from)
        .collect()
}

/// `encodeURIComponent`.
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// 32 random upper-hex digits, from the standard library's per-process random hasher keys and the clock.
fn device_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut bytes = Vec::with_capacity(16);
    for round in 0..2u64 {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(round);
        hasher.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos());
        bytes.extend_from_slice(&hasher.finish().to_be_bytes());
    }
    hex(&bytes, true)
}

/// The platform's `code`, wherever the answer puts it.
pub fn code_of(body: &Value) -> i64 {
    body.get("code").and_then(Value::as_i64).or_else(|| body.pointer("/data/code").and_then(Value::as_i64)).unwrap_or(0)
}

/// Fails on anything but code 200; 301 means the session is gone.
pub fn check(body: &Value) -> Result<(), OmniError> {
    match code_of(body) {
        200 => Ok(()),
        301 => Err(OmniError::SignedOut),
        code => Err(OmniError::Platform(code)),
    }
}
