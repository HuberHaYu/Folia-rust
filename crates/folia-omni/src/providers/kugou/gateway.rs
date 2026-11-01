// KuGou's lite-client gateway as KuGouMusicApi v1.6.0 speaks it: device cookies, signatures, AES and RSA.

use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use aes::Aes128;
use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use md5::{Digest, Md5};
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::OmniError;

const GATEWAY: &str = "https://gateway.kugou.com";
/// `liteAppid` and `liteClientver` (`config.json`): the bridge loads the package with `platform = 'lite'`.
pub const APPID: &str = "3116";
pub const CLIENTVER: &str = "11440";
pub const SRCAPPID: &str = "2919";
pub const ANDROID_SALT: &str = "LnT6xpN3khm36zse0QzvmgTZ3waWdRSA";
const WEB_SALT: &str = "NVPh5oo715z5DIWAeQlhMDsWXXQV4hwt";
const KEY_SALT: &str = "185672dd44712f60bb1736df5a377e82";
const USER_AGENT: &str = "Android15-1070-11083-46-0-DiscoveryDRADProtocol-wifi";
/// The modulus of `publicLiteRasKey`; its exponent is 65537.
const RSA_MODULUS: &str = "C40A2D0DA76511F3BB1CC2BBD3AFBD8BEA83B4D6B05B6C13EB8920C53F1AF7679B32BA0D0EDB843240EF1B836EFED3EE240734C14C1399FD6594D16AF22F52525D14D72E0155C6DCC8638D4F7BB94F3A0B1F4C29F991972F2A160A25EB0A9E724336BE7F69BBD319FFAB1C6DD8470B021DC434F3FABA89F4A2A01B33BDBDD08B";
/// `error_code` 20028 or this message: the device must register again (`isDeviceVerificationRequired`).
const VERIFY_CODE: i64 = 20028;

/// The bridge's cookie map: the lite device identity, then `dfid`, `token` and `userid`. Never logged.
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub guid: String,
    pub mid: String,
    #[serde(default)]
    pub dfid: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub userid: Option<String>,
}

impl Session {
    /// `createDeviceCookies`: a random GUID, and its MD5 read as a decimal number for the MID.
    pub fn device() -> Self {
        let guid = hex(&random_bytes(16), true);
        let digest = Md5::digest(guid.as_bytes());
        let mid = u128::from_be_bytes(digest.into()).to_string();
        Self { guid, mid, dfid: None, token: None, userid: None }
    }

    pub fn signed_in(&self) -> bool {
        self.token.as_deref().is_some_and(|token| !token.is_empty())
            && self.userid.as_deref().is_some_and(|id| !id.is_empty() && id != "0")
    }
}

/// How a request is signed (`encryptType`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sign {
    Android,
    Web,
}

pub enum Body {
    None,
    Json(Value),
    /// A ready string body, as the AES-wrapped endpoints send it.
    Text(String),
}

/// One request as a KuGouMusicApi module describes it to `createRequest`.
pub struct Request {
    pub post: bool,
    pub base: &'static str,
    pub path: &'static str,
    pub params: Vec<(&'static str, String)>,
    pub body: Body,
    pub router: Option<&'static str>,
    pub sign: Sign,
    /// `clearDefaultParams`: only the module's own parameters.
    pub bare: bool,
    /// `encryptKey`: adds `key` signed over the hash.
    pub keyed: bool,
    pub headers: Vec<(&'static str, &'static str)>,
    /// A `dfid` used in place of the session's (`song_url` gives a random one when the session has none).
    pub dfid: Option<String>,
}

impl Request {
    pub fn get(path: &'static str) -> Self {
        Self {
            post: false,
            base: GATEWAY,
            path,
            params: Vec::new(),
            body: Body::None,
            router: None,
            sign: Sign::Android,
            bare: false,
            keyed: false,
            headers: Vec::new(),
            dfid: None,
        }
    }

    pub fn post(path: &'static str, body: Value) -> Self {
        Self { post: true, body: Body::Json(body), ..Self::get(path) }
    }

    pub fn base(mut self, base: &'static str) -> Self {
        self.base = base;
        self
    }

    pub fn router(mut self, router: &'static str) -> Self {
        self.router = Some(router);
        self
    }

    pub fn param(mut self, key: &'static str, value: impl ToString) -> Self {
        self.params.push((key, value.to_string()));
        self
    }
}

pub struct Client {
    http: reqwest::Client,
    session: Mutex<Session>,
    registering: tokio::sync::Mutex<()>,
}

impl Client {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http, session: Mutex::new(Session::device()), registering: tokio::sync::Mutex::new(()) }
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn session(&self) -> Session {
        self.session.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn set_session(&self, session: Session) {
        *self.session.lock().unwrap_or_else(PoisonError::into_inner) = session;
    }

    pub fn update(&self, change: impl FnOnce(&mut Session)) {
        change(&mut self.session.lock().unwrap_or_else(PoisonError::into_inner));
    }

    /// `performRequest`: registers the device first, and once more when KuGou asks to verify it.
    pub async fn call(&self, request: impl Fn() -> Request) -> Result<Value, OmniError> {
        self.ensure_registered(false).await?;
        match self.send(request()).await {
            Err(OmniError::Platform(VERIFY_CODE)) => {
                tracing::info!("kugou asked to verify the device; registering again");
                self.ensure_registered(true).await?;
                self.send(request()).await
            }
            other => other,
        }
    }

    /// `ensureRegistered`: `register_dev` once per device, one at a time.
    async fn ensure_registered(&self, force: bool) -> Result<(), OmniError> {
        let _turn = self.registering.lock().await;
        if !force && self.session().dfid.is_some() {
            return Ok(());
        }
        self.update(|session| session.dfid = None);
        let dfid = register_dev(self).await?;
        self.update(|session| session.dfid = Some(dfid));
        Ok(())
    }

    /// `createRequest`: default parameters, the signature, the headers, then KuGou's success rule.
    pub async fn send(&self, request: Request) -> Result<Value, OmniError> {
        let bytes = self.send_raw(request).await?;
        let body: Value = serde_json::from_slice(&bytes).map_err(|err| OmniError::Response(err.to_string()))?;
        refusal(&body).map_or(Ok(body), Err)
    }

    pub async fn send_raw(&self, request: Request) -> Result<Vec<u8>, OmniError> {
        let session = self.session();
        let dfid = request.dfid.clone().or(session.dfid.clone()).unwrap_or_else(|| "-".to_owned());
        let clienttime = now().as_secs().to_string();
        let mut params: Vec<(String, String)> = Vec::new();
        if !request.bare {
            params.extend([
                ("dfid".to_owned(), dfid.clone()),
                ("mid".to_owned(), session.mid.clone()),
                ("uuid".to_owned(), "-".to_owned()),
                ("appid".to_owned(), APPID.to_owned()),
                ("clientver".to_owned(), CLIENTVER.to_owned()),
                ("clienttime".to_owned(), clienttime.clone()),
            ]);
            if let Some(token) = session.token.as_ref().filter(|token| !token.is_empty()) {
                params.push(("token".to_owned(), token.clone()));
            }
            if let Some(userid) = session.userid.as_ref().filter(|id| !id.is_empty() && *id != "0") {
                params.push(("userid".to_owned(), userid.clone()));
            }
        }
        for (key, value) in &request.params {
            match params.iter_mut().find(|(name, _)| name == key) {
                Some(slot) => slot.1 = value.clone(),
                None => params.push(((*key).to_owned(), value.clone())),
            }
        }
        if request.keyed {
            let field = |name: &str| params.iter().find(|(key, _)| key == name).map(|(_, v)| v.clone());
            let key = sign_key(
                &field("hash").unwrap_or_default(),
                &field("mid").unwrap_or_default(),
                &field("userid").unwrap_or_else(|| "0".to_owned()),
                &field("appid").unwrap_or_else(|| APPID.to_owned()),
            );
            params.push(("key".to_owned(), key));
        }
        let data = match &request.body {
            Body::None => String::new(),
            Body::Json(value) => value.to_string(),
            Body::Text(text) => text.clone(),
        };
        let signature = match request.sign {
            Sign::Android => sign_android(&params, &data),
            Sign::Web => sign_web(&params),
        };
        params.push(("signature".to_owned(), signature));
        let header_time = params.iter().find(|(key, _)| key == "clienttime").map_or(clienttime, |(_, v)| v.clone());

        let url = format!("{}{}{}", request.base, if request.path.starts_with('/') { "" } else { "/" }, request.path);
        let url = with_query(&url, &params)?;
        let mut builder = if request.post { self.http.post(url) } else { self.http.get(url) };
        builder = builder
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header("dfid", dfid)
            .header("clienttime", header_time)
            .header("mid", session.mid)
            .header("kg-rc", "1")
            .header("kg-thash", "5d816a0")
            .header("kg-rec", "1")
            .header("kg-rf", "B9EDA08A64250DEFFBCADDEE00F8F25F");
        if let Some(router) = request.router {
            builder = builder.header("x-router", router);
        }
        for (name, value) in request.headers {
            builder = builder.header(name, value);
        }
        builder = match request.body {
            Body::None => builder,
            Body::Json(_) => builder.header(reqwest::header::CONTENT_TYPE, "application/json").body(data),
            Body::Text(_) => {
                builder.header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded").body(data)
            }
        };
        let response = builder.send().await?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            return Err(OmniError::SignedOut);
        }
        Ok(response.error_for_status()?.bytes().await?.to_vec())
    }
}

/// KuGou's failure rule (`status === 0` or a non-zero `error_code`); `error_code` 152 means the sign-in is gone.
pub fn refusal(body: &Value) -> Option<OmniError> {
    let number = |key: &str| {
        body.get(key).and_then(|value| value.as_i64().or_else(|| value.as_str().and_then(|text| text.parse().ok())))
    };
    let message = ["error", "error_msg", "msg"].iter().find_map(|key| body.get(*key).and_then(Value::as_str));
    let verify = number("errcode").or_else(|| number("error_code")) == Some(VERIFY_CODE);
    if verify || message.is_some_and(|text| text.contains("本次请求需要验证")) {
        return Some(OmniError::Platform(VERIFY_CODE));
    }
    let status_zero = body.get("status").and_then(Value::as_i64) == Some(0);
    match number("error_code").filter(|code| *code != 0) {
        Some(152) => Some(OmniError::SignedOut),
        Some(code) => Some(OmniError::Platform(code)),
        None if status_zero => Some(OmniError::Platform(0)),
        None => None,
    }
}

/// `register_dev`: the lite client's sensor report, AES-wrapped, its key sent RSA-encrypted; answers a `dfid`.
async fn register_dev(client: &Client) -> Result<String, OmniError> {
    let session = client.session();
    let report = serde_json::json!({
        "availableRamSize": 4983533568u64, "availableRomSize": 48114719, "availableSDSize": 48114717,
        "basebandVer": "", "batteryLevel": 100, "batteryStatus": 3, "brand": "Redmi", "buildSerial": "unknown",
        "device": "marble", "imei": session.guid, "imsi": "", "manufacturer": "Xiaomi", "uuid": session.guid,
        "accelerometer": false, "accelerometerValue": "", "gravity": false, "gravityValue": "",
        "gyroscope": false, "gyroscopeValue": "", "light": false, "lightValue": "", "magnetic": false,
        "magneticValue": "", "orientation": false, "orientationValue": "", "pressure": false,
        "pressureValue": "", "step_counter": false, "step_counterValue": "", "temperature": false,
        "temperatureValue": "",
    });
    let (key, sealed) = playlist_aes_encrypt(&report.to_string());
    let p = rsa_pkcs1(&key_envelope(&key, &session))?;
    let request = Request {
        post: true,
        base: "https://userservice.kugou.com",
        path: "/risk/v2/r_register_dev",
        params: vec![("part", "1".to_owned()), ("platid", "1".to_owned()), ("p", p)],
        body: Body::Text(sealed),
        ..Request::get("/")
    };
    let raw = client.send_raw(request).await?;
    let body = playlist_aes_decrypt(&key, &STANDARD.encode(raw))?;
    let dfid = body
        .pointer("/data/dfid")
        .and_then(Value::as_str)
        .filter(|_| body.get("status").and_then(Value::as_i64) == Some(1))
        .filter(|dfid| !dfid.is_empty())
        .ok_or_else(|| OmniError::Response("KuGou device registration did not return a dfid".to_owned()))?;
    tracing::info!("kugou device registered");
    Ok(dfid.to_owned())
}

/// `{ aes, uid, token }`, in the order `rsaEncrypt2` serialises it.
pub fn key_envelope(key: &str, session: &Session) -> String {
    #[derive(Serialize)]
    struct Envelope<'a> {
        aes: &'a str,
        uid: Value,
        token: &'a str,
    }
    let uid = match session.userid.as_deref().filter(|id| !id.is_empty()) {
        Some(id) => Value::from(id),
        None => Value::from(0),
    };
    let envelope = Envelope { aes: key, uid, token: session.token.as_deref().unwrap_or_default() };
    serde_json::to_string(&envelope).unwrap_or_default()
}

/// `signatureAndroidParams`: MD5 of the salt, the sorted `key=value` pairs, the body and the salt.
fn sign_android(params: &[(String, String)], data: &str) -> String {
    let mut pairs: Vec<&(String, String)> = params.iter().collect();
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let joined: String = pairs.iter().map(|(key, value)| format!("{key}={value}")).collect();
    md5_hex(&format!("{ANDROID_SALT}{joined}{data}{ANDROID_SALT}"))
}

/// `signatureWebParams`: the `key=value` strings themselves sorted.
fn sign_web(params: &[(String, String)]) -> String {
    let mut pairs: Vec<String> = params.iter().map(|(key, value)| format!("{key}={value}")).collect();
    pairs.sort();
    md5_hex(&format!("{WEB_SALT}{}{WEB_SALT}", pairs.concat()))
}

/// `signKey` for the lite client.
fn sign_key(hash: &str, mid: &str, userid: &str, appid: &str) -> String {
    md5_hex(&format!("{hash}{KEY_SALT}{appid}{mid}{userid}"))
}

/// `signParamsKey` for the lite client.
pub fn sign_params_key(data: &str) -> String {
    md5_hex(&format!("{APPID}{ANDROID_SALT}{CLIENTVER}{data}"))
}

pub fn md5_hex(text: &str) -> String {
    hex(&Md5::digest(text.as_bytes()), false)
}

/// `playlistAesEncrypt`: a six-character key, AES-128-CBC keyed and seeded by halves of its MD5, base64.
pub fn playlist_aes_encrypt(text: &str) -> (String, String) {
    let key = random_string(6).to_lowercase();
    let digest = md5_hex(&key);
    let sealed = cbc_encrypt(&digest.as_bytes()[..16], &digest.as_bytes()[16..32], text.as_bytes());
    (key, STANDARD.encode(sealed))
}

/// `playlistAesDecrypt`, read as JSON.
pub fn playlist_aes_decrypt(key: &str, sealed: &str) -> Result<Value, OmniError> {
    let digest = md5_hex(key);
    let bytes = STANDARD.decode(sealed).map_err(|err| OmniError::Response(format!("kugou answer: {err}")))?;
    let plain = cbc_decrypt(&digest.as_bytes()[..16], &digest.as_bytes()[16..32], &bytes)
        .ok_or_else(|| OmniError::Response("kugou answer does not decrypt".to_owned()))?;
    serde_json::from_slice(&plain).map_err(|err| OmniError::Response(err.to_string()))
}

fn cbc_encrypt(key: &[u8], iv: &[u8], plain: &[u8]) -> Vec<u8> {
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut message = plain.to_vec();
    let pad = 16 - message.len() % 16;
    message.extend(std::iter::repeat_n(pad as u8, pad));
    let mut previous = [0u8; 16];
    previous.copy_from_slice(iv);
    for block in message.as_chunks_mut::<16>().0 {
        for (byte, prior) in block.iter_mut().zip(previous) {
            *byte ^= prior;
        }
        cipher.encrypt_block(GenericArray::from_mut_slice(&mut block[..]));
        previous = *block;
    }
    message
}

fn cbc_decrypt(key: &[u8], iv: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
    if sealed.is_empty() || !sealed.len().is_multiple_of(16) {
        return None;
    }
    let cipher = Aes128::new(GenericArray::from_slice(key));
    let mut message = sealed.to_vec();
    let mut previous = [0u8; 16];
    previous.copy_from_slice(iv);
    for block in message.as_chunks_mut::<16>().0 {
        let current = *block;
        cipher.decrypt_block(GenericArray::from_mut_slice(&mut block[..]));
        for (byte, prior) in block.iter_mut().zip(previous) {
            *byte ^= prior;
        }
        previous = current;
    }
    let pad = usize::from(*message.last()?);
    if pad == 0 || pad > 16 || pad > message.len() {
        return None;
    }
    message.truncate(message.len() - pad);
    Some(message)
}

/// `rsaEncrypt2`: RSAES-PKCS1-v1_5 under the lite key, lower hex.
pub fn rsa_pkcs1(text: &str) -> Result<String, OmniError> {
    let size = RSA_MODULUS.len() / 2;
    let data = text.as_bytes();
    if data.len() + 11 > size {
        return Err(OmniError::Response("kugou key envelope too long".to_owned()));
    }
    let mut block = vec![0u8, 2];
    let mut filler = Vec::new();
    while filler.len() < size - 3 - data.len() {
        filler.extend(random_bytes(32).into_iter().filter(|byte| *byte != 0));
    }
    filler.truncate(size - 3 - data.len());
    block.extend(filler);
    block.push(0);
    block.extend_from_slice(data);
    Ok(rsa_raw(&block, size))
}

/// `cryptoRSAEncrypt`: the text zero-padded on the right to the key's size, raised without padding, lower hex.
pub fn rsa_zero_padded(text: &str) -> Result<String, OmniError> {
    let size = RSA_MODULUS.len() / 2;
    let mut block = text.as_bytes().to_vec();
    if block.len() > size {
        return Err(OmniError::Response("kugou sealed text too long".to_owned()));
    }
    block.resize(size, 0);
    Ok(rsa_raw(&block, size))
}

fn rsa_raw(block: &[u8], size: usize) -> String {
    let modulus = BigUint::parse_bytes(RSA_MODULUS.as_bytes(), 16).unwrap_or_default();
    let sealed = BigUint::from_bytes_be(block).modpow(&BigUint::from(65537u32), &modulus);
    format!("{:0>width$}", sealed.to_str_radix(16), width = size * 2)
}

/// `randomString`: upper letters and digits.
pub fn random_string(len: usize) -> String {
    const POOL: &[u8] = b"1234567890ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    random_bytes(len).into_iter().map(|byte| char::from(POOL[usize::from(byte) % POOL.len()])).collect()
}

/// Random bytes from the standard library's per-process random hasher keys and the clock.
fn random_bytes(len: usize) -> Vec<u8> {
    use std::hash::{BuildHasher, Hasher};
    let mut bytes = Vec::with_capacity(len + 8);
    let mut round = 0u64;
    while bytes.len() < len {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(round);
        hasher.write_u128(now().as_nanos());
        bytes.extend_from_slice(&hasher.finish().to_be_bytes());
        round += 1;
    }
    bytes.truncate(len);
    bytes
}

/// `url` with `params` as its query string.
pub fn with_query<K: AsRef<str>, V: AsRef<str>>(url: &str, params: &[(K, V)]) -> Result<reqwest::Url, OmniError> {
    let pairs = params.iter().map(|(key, value)| (key.as_ref(), value.as_ref()));
    reqwest::Url::parse_with_params(url, pairs).map_err(|err| OmniError::Response(format!("kugou address: {err}")))
}

pub fn now() -> std::time::Duration {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()
}

pub fn hex(bytes: &[u8], upper: bool) -> String {
    let digits: &[u8; 16] = if upper { b"0123456789ABCDEF" } else { b"0123456789abcdef" };
    bytes
        .iter()
        .flat_map(|byte| [digits[usize::from(byte >> 4)], digits[usize::from(byte & 15)]])
        .map(char::from)
        .collect()
}
