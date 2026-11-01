// Qishui's car protocol (qishui-music-api 0.2.1): common parameters, the public client token, and the
// ticket-guard credential whose P-256 key signs every account request.

use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, KeyPair};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::OmniError;

pub const UA: &str = "LunaOpenSDK/1.0.10";
pub const PREFIX: &str = "/api/luna/v1/platform";
pub const CLIENT_KEY: &str = "aw4bbxfujo741d09";
const COMMON: [(&str, &str); 18] = [
    ("aid", "765213"),
    ("app_id", "765213"),
    ("app_name", "byte_autoservice_lite"),
    ("version_code", "100010"),
    ("version_name", "1.0.10"),
    ("channel", "bydSong"),
    ("sdk_aid", "753999"),
    ("caller_name", "luna_open_sdk"),
    ("sdk_version_code", "100000100"),
    ("sdk_version_name", "0.0.1"),
    ("sdk_channel", "Test"),
    ("vehicle_type", "BYD"),
    ("device_brand", "BYD"),
    ("device_type", "BYD"),
    ("device_platform", "android"),
    ("resolution", "1920x1080"),
    ("os_version", "13"),
    ("ac", "wifi"),
];
/// `AUTH_CODES`: the sign-in is gone.
const AUTH_CODES: [i64; 4] = [28001003, 28001007, 28001008, 10007];
/// The public client token ran out.
const TOKEN_EXPIRED: i64 = 28001008;
/// `RETRYABLE_READS`: only these replay with a new client token.
const RETRYABLE: [&str; 8] = [
    "/search/",
    "/playlist/detail/",
    "/media/mget/",
    "/media/mcheck/",
    "/artist/detail/",
    "/artist/tracks/",
    "/feed/mode/",
    "/feed/song-tab/",
];

/// The ticket-guard credential and its account. Never logged.
#[derive(Clone, Serialize, Deserialize)]
pub struct Credential {
    pub ticket: String,
    /// The PKCS#8 P-256 key, base64.
    pub guard_key: String,
    pub guard_cert: String,
    pub guard_ts_sign: String,
    pub access_code: String,
    pub open_id: String,
    /// Milliseconds since the epoch.
    pub expires_at: u64,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub nickname: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub device_id: String,
    #[serde(default)]
    pub credential: Option<Credential>,
}

pub struct Client {
    http: reqwest::Client,
    session: Mutex<Session>,
    token: tokio::sync::Mutex<Option<String>>,
    pub rng: SystemRandom,
}

impl Client {
    pub fn new(http: reqwest::Client) -> Self {
        let rng = SystemRandom::new();
        let device_id = device_id(&rng);
        Self {
            http,
            session: Mutex::new(Session { device_id, credential: None }),
            token: tokio::sync::Mutex::new(None),
            rng,
        }
    }

    pub fn session(&self) -> Session {
        self.session.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn set_session(&self, session: Session) {
        *self.session.lock().unwrap_or_else(PoisonError::into_inner) = session;
    }

    pub fn set_credential(&self, credential: Option<Credential>) {
        self.session.lock().unwrap_or_else(PoisonError::into_inner).credential = credential;
    }

    /// `query`: the common parameters for this device, then `extra`.
    pub fn query(&self, url: &str, extra: &[(&str, String)]) -> Result<reqwest::Url, OmniError> {
        let device = self.session().device_id;
        let mut nonce = [0u8; 2];
        let _ = self.rng.fill(&mut nonce);
        let mut params: Vec<(String, String)> =
            COMMON.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        params.extend([
            ("device_id".to_owned(), device.clone()),
            ("did".to_owned(), device.clone()),
            ("iid".to_owned(), device.clone()),
            ("vehicle_id".to_owned(), format!("qishui-{device}")),
            ("_timestamp".to_owned(), now_millis().div_euclid(1000).to_string()),
            ("_nonce".to_owned(), u16::from_be_bytes(nonce).to_string()),
        ]);
        for (key, value) in extra {
            match params.iter_mut().find(|(name, _)| name == key) {
                Some(slot) => slot.1 = value.clone(),
                None => params.push(((*key).to_owned(), value.clone())),
            }
        }
        reqwest::Url::parse_with_params(url, &params)
            .map_err(|err| OmniError::Response(format!("qishui address: {err}")))
    }

    /// `http`: JSON or a form, the car user agent, and `checked` on the answer; the headers come back too.
    pub async fn http(
        &self,
        url: reqwest::Url,
        body: Option<Body>,
        headers: Vec<(&'static str, String)>,
    ) -> Result<(Value, reqwest::header::HeaderMap), OmniError> {
        let mut request = match body {
            None => self.http.get(url),
            Some(Body::Json(value)) => {
                self.http.post(url).header(reqwest::header::CONTENT_TYPE, "application/json").body(value.to_string())
            }
            Some(Body::Form(fields)) => self.http.post(url).form(&fields),
        };
        request = request.header(reqwest::header::USER_AGENT, UA);
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let response = request.send().await?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(OmniError::SignedOut);
        }
        let response = response.error_for_status()?;
        let headers = response.headers().clone();
        let data: Value = response.json().await?;
        checked(&data)?;
        Ok((data, headers))
    }

    /// `clientToken`: one public token at a time, asked again once it runs out.
    async fn client_token(&self, renew: bool) -> Result<String, OmniError> {
        let mut kept = self.token.lock().await;
        if let (Some(token), false) = (kept.as_ref(), renew) {
            return Ok(token.clone());
        }
        let url = self.query(
            &format!("https://open.qishui.com{PREFIX}/client/token/get/"),
            &[("trigger_type", "cold_start".to_owned())],
        )?;
        let (data, _) = self.http(url, None, Vec::new()).await?;
        let token = data
            .get("client_token")
            .and_then(Value::as_str)
            .filter(|token| !token.is_empty())
            .ok_or_else(|| OmniError::Response("Client token is missing".to_owned()))?
            .to_owned();
        *kept = Some(token.clone());
        Ok(token)
    }

    /// `platform`: a car read, with the account's guard when `account` (or with the public token otherwise).
    pub async fn platform(&self, path: &str, body: Value, account: bool) -> Result<Value, OmniError> {
        let credential = self.session().credential;
        if account && credential.is_none() {
            return Err(OmniError::SignedOut);
        }
        let mut body = body;
        if let Some(map) = body.as_object_mut() {
            let user = credential.as_ref().and_then(|c| c.user_id.clone());
            map.insert("common_params".to_owned(), user.map_or_else(|| json!({}), |id| json!({ "user_id": id })));
        }
        let url = self.query(&format!("https://open.douyin.com{PREFIX}{path}"), &[])?;
        if let Some(credential) = credential.as_ref().filter(|_| account) {
            let headers = guard_headers(credential, path, &self.rng)?;
            return match self.http(url, Some(Body::Json(body)), headers).await {
                Err(OmniError::Platform(TOKEN_EXPIRED)) => Err(OmniError::SignedOut),
                other => data_of(other?.0),
            };
        }
        let mut renew = false;
        loop {
            let token = self.client_token(renew).await?;
            match self.http(url.clone(), Some(Body::Json(body.clone())), vec![("access-token", token)]).await {
                Err(OmniError::Platform(TOKEN_EXPIRED)) if !renew && RETRYABLE.contains(&path) => renew = true,
                Err(OmniError::Platform(TOKEN_EXPIRED)) => return Err(OmniError::Platform(TOKEN_EXPIRED)),
                other => return data_of(other?.0),
            }
        }
    }

    /// A read as `platform` with a credential that is not kept yet (the sign-in's own account read).
    pub async fn platform_as(&self, path: &str, credential: &Credential) -> Result<Value, OmniError> {
        let url = self.query(&format!("https://open.douyin.com{PREFIX}{path}"), &[])?;
        let headers = guard_headers(credential, path, &self.rng)?;
        let body = json!({ "common_params": {} });
        data_of(self.http(url, Some(Body::Json(body)), headers).await?.0)
    }
}

pub enum Body {
    Json(Value),
    Form(Vec<(&'static str, String)>),
}

fn data_of(data: Value) -> Result<Value, OmniError> {
    match data.get("data") {
        Some(inner) if inner.is_object() => Ok(inner.clone()),
        _ => Err(OmniError::Response("Qishui data is missing".to_owned())),
    }
}

/// `checked`: `err_no` or `data.error_code`.
fn checked(data: &Value) -> Result<(), OmniError> {
    if !data.is_object() {
        return Err(OmniError::Response("Invalid Qishui response".to_owned()));
    }
    let number = |value: Option<&Value>| value.and_then(|v| v.as_i64().or_else(|| v.as_str()?.parse().ok()));
    let code = number(data.get("err_no")).or_else(|| number(data.pointer("/data/error_code"))).unwrap_or(0);
    match code {
        0 => Ok(()),
        code if AUTH_CODES.contains(&code) && code != TOKEN_EXPIRED => Err(OmniError::SignedOut),
        code => Err(OmniError::Platform(code)),
    }
}

/// `qishuiGuardHeaders`: the ticket, the certificate, and a signature over `ticket`, `path` and `timestamp`.
fn guard_headers(
    credential: &Credential,
    path: &str,
    rng: &SystemRandom,
) -> Result<Vec<(&'static str, String)>, OmniError> {
    let pkcs8 =
        STANDARD.decode(&credential.guard_key).map_err(|_| OmniError::Response("invalid guard key".to_owned()))?;
    let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &pkcs8, rng)
        .map_err(|_| OmniError::Response("invalid guard key".to_owned()))?;
    let timestamp = now_millis() / 1000;
    let content = format!("ticket={}&path={PREFIX}{path}&timestamp={timestamp}", credential.ticket);
    let signature =
        key.sign(rng, content.as_bytes()).map_err(|_| OmniError::Response("guard signing failed".to_owned()))?;
    let data = json!({
        "ts_sign": credential.guard_ts_sign,
        "req_content": "ticket,path,timestamp",
        "req_sign": STANDARD.encode(signature.as_ref()),
        "timestamp": timestamp,
    });
    Ok(vec![
        ("access-token", credential.ticket.clone()),
        ("bd-ticket-guard-version", "1".to_owned()),
        ("bd-ticket-guard-client-cert", credential.guard_cert.clone()),
        ("bd-ticket-guard-client-data", STANDARD.encode(data.to_string())),
    ])
}

/// `createQishuiGuard`: a fresh P-256 key and its PKCS#10 request (CN `bd-ticket-guard`), PEM in base64.
pub fn create_guard(rng: &SystemRandom) -> Result<(String, String), OmniError> {
    let fail = |_| OmniError::Response("guard key generation failed".to_owned());
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, rng).map_err(fail)?;
    let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), rng)
        .map_err(|_| OmniError::Response("guard key generation failed".to_owned()))?;
    let spki = der(
        0x30,
        &[
            &der(0x30, &[&der(6, &[&hex("2a8648ce3d0201")]), &der(6, &[&hex("2a8648ce3d030107")])]),
            &der(3, &[&[0], key.public_key().as_ref()]),
        ],
    );
    let names: Vec<Vec<u8>> = [(3u8, "bd-ticket-guard"), (11, ""), (10, "")]
        .iter()
        .map(|(id, value)| der(0x31, &[&der(0x30, &[&der(6, &[&[0x55, 4, *id]]), &der(12, &[value.as_bytes()])])]))
        .collect();
    let subject = der(0x30, &names.iter().map(Vec::as_slice).collect::<Vec<_>>());
    let info = der(0x30, &[&der(2, &[&[0]]), &subject, &spki, &der(0xa0, &[])]);
    let algorithm = der(0x30, &[&der(6, &[&hex("2a8648ce3d040302")])]);
    let signature = key.sign(rng, &info).map_err(fail)?;
    let request = der(0x30, &[&info, &algorithm, &der(3, &[&[0], signature.as_ref()])]);
    let body = STANDARD.encode(&request);
    let lines: Vec<&str> = body.as_bytes().chunks(64).filter_map(|chunk| std::str::from_utf8(chunk).ok()).collect();
    let pem = format!("-----BEGIN CERTIFICATE REQUEST-----\n{}\n-----END CERTIFICATE REQUEST-----\n", lines.join("\n"));
    Ok((STANDARD.encode(pkcs8.as_ref()), STANDARD.encode(pem)))
}

/// `der`: a tag, a short or one- or two-byte long length, and the values.
fn der(tag: u8, values: &[&[u8]]) -> Vec<u8> {
    let value: Vec<u8> = values.concat();
    let mut out = vec![tag];
    match value.len() {
        len if len < 128 => out.push(len as u8),
        len if len < 256 => out.extend([0x81, len as u8]),
        len => out.extend([0x82, (len >> 8) as u8, (len & 255) as u8]),
    }
    out.extend(value);
    out
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len() / 2).filter_map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()).collect()
}

/// `readQishuiGuard`: the ticket, the time-stamp signature and the certificate from the server's guard data.
pub fn read_guard(raw: &str) -> Result<(String, String, String), OmniError> {
    let invalid = || OmniError::Response("invalid guard response".to_owned());
    let bytes = STANDARD.decode(raw).map_err(|_| invalid())?;
    let data: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    let field = |key: &str| {
        data.get(key).and_then(Value::as_str).filter(|v| !v.is_empty() && v.len() <= 16384).map(str::to_owned)
    };
    let (Some(ticket), Some(ts_sign), Some(cert)) = (field("ticket"), field("ts_sign"), field("client_cert")) else {
        return Err(invalid());
    };
    let cert = if cert.contains("-----BEGIN") { STANDARD.encode(cert) } else { cert };
    Ok((ticket, ts_sign, cert))
}

/// `qishuiCredentialExpiry`: milliseconds, seconds, a lifetime in seconds, or six hours.
pub fn expiry(raw: Option<&Value>) -> u64 {
    let value = raw.and_then(|v| v.as_f64().or_else(|| v.as_str()?.parse().ok())).unwrap_or(0.0);
    let now = now_millis();
    if value > 1e12 {
        value as u64
    } else if value > 1e9 {
        (value * 1000.0) as u64
    } else if value > 0.0 {
        now + (value * 1000.0) as u64
    } else {
        now + 6 * 3_600_000
    }
}

/// `createQishuiDeviceId`: nineteen digits.
fn device_id(rng: &SystemRandom) -> String {
    let mut bytes = [0u8; 8];
    let _ = rng.fill(&mut bytes);
    (u64::from_be_bytes(bytes) % 9_000_000_000_000_000_000 + 1_000_000_000_000_000_000).to_string()
}

pub fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

/// `randomUUID`.
pub fn uuid(rng: &SystemRandom) -> String {
    let mut bytes = [0u8; 16];
    let _ = rng.fill(&mut bytes);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let text: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &text[..8], &text[8..12], &text[12..16], &text[16..20], &text[20..])
}
