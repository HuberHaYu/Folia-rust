// QQ Music's musicu gateway in its three faces: the lite app's anonymous one, the web's, and the Android app's with
// a QIMEI-bootstrapped device and the signed-in credential (qq-music-api 3.3.3, `musicuProtocol` and `bootstrap`).

use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use aes::Aes128;
use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockEncrypt, KeyInit};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use md5::{Digest, Md5};
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::OmniError;

pub const MUSICU: &str = "https://u.y.qq.com/cgi-bin/musicu.fcg";
const QIMEI_URL: &str = "https://api.tencentmusic.com/tme/trpc/proxy";
const QIMEI_SECRET: &str = "ZdJqM15EeO2zWc08";
const QIMEI_APP_KEY: &str = "0AND0HD6FE4HY80F";
const QIMEI_TTL_SECS: u64 = 24 * 60 * 60;
const CHANNEL_ID: &str = "10003505";
const QIMEI_RSA_MODULUS: &str = "c4231830a2eb5fc2827170641e79d80fec51bda9a22e4b4ab37d1f205a4ae44d928cda25879f66a3429051663312a127faf8a246bdaaf63918417e90d7c95b5908aa6a2d0f852e4a6770294a548ac1c2fe8f1f252fb826f4ac86ab9a00e7ce47d002a56e7c4b51eb889acc60ca6adbc9f72e81f4d31b1dd7464805264530ab1d";
/// `CREDENTIAL_REFRESH_SAFETY_CODES`: the musicu codes that refuse the credential itself.
const REFUSED_CREDENTIAL: [i64; 3] = [1000, 104401, 104400];

/// `createAndroidDevice`: a synthetic phone, plus its QIMEI and musicu session once bootstrapped.
#[derive(Clone, Serialize, Deserialize)]
pub struct Device {
    display: String,
    fingerprint: String,
    proc_version: String,
    imei: String,
    android_id: String,
    open_udid: String,
    #[serde(default)]
    qimei: Option<String>,
    #[serde(default)]
    qimei36: Option<String>,
    #[serde(default)]
    qimei_saved_at: Option<u64>,
    #[serde(default)]
    session_uid: Option<String>,
    #[serde(default)]
    session_sid: Option<String>,
}

const MODEL: &str = "MI 6";
const BRAND: &str = "Xiaomi";
const OS_RELEASE: &str = "10";
const SDK: u32 = 29;

impl Device {
    fn new() -> Self {
        Self {
            display: format!("QMAPI.{}.001", random_digits(6)),
            fingerprint: format!("xiaomi/iarim/sagit:10/eomam.200122.001/{}:user/release-keys", random_digits(7)),
            proc_version: format!("Linux 5.4.0-54-generic-{} (android-build@google.com)", random_hex(8)),
            imei: random_imei(),
            android_id: random_hex(16),
            open_udid: random_hex(32),
            qimei: None,
            qimei36: None,
            qimei_saved_at: None,
            session_uid: None,
            session_sid: None,
        }
    }
}

/// The QQ Music credential the sign-in exchange answers (`musicid`, `musickey`, `loginType`). Never logged.
#[derive(Clone, Serialize, Deserialize)]
pub struct Credential {
    pub musicid: String,
    pub musickey: String,
    pub login_type: i64,
    #[serde(default)]
    pub encrypt_uin: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub device: Device,
    #[serde(default)]
    pub credential: Option<Credential>,
}

pub struct Client {
    http: reqwest::Client,
    session: Mutex<Session>,
    bootstrapping: tokio::sync::Mutex<()>,
}

impl Client {
    pub fn new(http: reqwest::Client) -> Self {
        Self {
            http,
            session: Mutex::new(Session { device: Device::new(), credential: None }),
            bootstrapping: tokio::sync::Mutex::new(()),
        }
    }

    pub fn session(&self) -> Session {
        self.session.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn set_session(&self, session: Session) {
        *self.session.lock().unwrap_or_else(PoisonError::into_inner) = session;
    }

    pub fn credential(&self) -> Option<Credential> {
        self.session().credential
    }

    pub fn set_credential(&self, credential: Option<Credential>) {
        self.session.lock().unwrap_or_else(PoisonError::into_inner).credential = credential;
    }

    /// `requestQQ` of `qqLyricProvider`: the lite app's anonymous face, `{ comm, request }`.
    pub async fn light(&self, module: &str, method: &str, param: Value) -> Result<Value, OmniError> {
        let payload = json!({
            "comm": {
                "ct": 11, "cv": "1003006", "v": "1003006", "os_ver": "15", "phonetype": "24122RKC7C",
                "rom": "Redmi/miro/miro:15/AE3A.240806.005/OS2.0.102.0.VOMCNXM:user/release-keys",
                "tmeAppID": "qqmusiclight", "nettype": "NETWORK_WIFI", "udid": "0", "uid": "0",
            },
            "request": { "method": method, "module": module, "param": param },
        });
        let answer: Value = self
            .http
            .post(MUSICU)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::COOKIE, "tmeLoginType=-1;")
            .header(reqwest::header::USER_AGENT, "okhttp/3.14.9")
            .body(payload.to_string())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let code = answer.get("code").and_then(Value::as_i64).unwrap_or(-1);
        let inner = answer.pointer("/request/code").and_then(Value::as_i64).unwrap_or(-1);
        if code != 0 || inner != 0 {
            return Err(OmniError::Platform(if code != 0 { code } else { inner }));
        }
        Ok(answer.pointer("/request/data").cloned().unwrap_or(Value::Null))
    }

    /// `createSessionMusicuClient(null)`: the anonymous `{ ct: 24, cv: 0 }` face.
    pub async fn anonymous(&self, module: &str, method: &str, param: Value) -> Result<Value, OmniError> {
        let payload =
            json!({ "comm": { "ct": 24, "cv": 0 }, "req_0": { "module": module, "method": method, "param": param } });
        let request = self.http.post(MUSICU).header(reqwest::header::CONTENT_TYPE, "application/json");
        unpack(request.body(payload.to_string()).send().await?.error_for_status()?.json().await?)
    }

    /// `callMusicu` with the credential: the Android comm, its user agent and the credential's cookie.
    pub async fn android(&self, module: &str, method: &str, param: Value) -> Result<Value, OmniError> {
        let credential = self.credential().ok_or(OmniError::SignedOut)?;
        if self.session().device.qimei.is_none() {
            self.bootstrap(false).await?;
        }
        let device = self.session().device;
        let payload = json!({ "comm": android_comm(&device, Some(&credential)), "req_0": { "module": module, "method": method, "param": param } });
        match self.post_android(&payload, Some(&credential)).await {
            Err(OmniError::Platform(code)) if REFUSED_CREDENTIAL.contains(&code) => Err(OmniError::SignedOut),
            other => other,
        }
    }

    async fn post_android(&self, payload: &Value, credential: Option<&Credential>) -> Result<Value, OmniError> {
        let mut request = self
            .http
            .post(MUSICU)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(reqwest::header::USER_AGENT, format!("QQMusic 14090008(android {OS_RELEASE})"));
        if let Some(credential) = credential {
            request = request.header(reqwest::header::COOKIE, credential_cookie(credential));
        }
        unpack(request.body(payload.to_string()).send().await?.error_for_status()?.json().await?)
    }

    /// `ensureQimei` then `refreshAndroidSession`, as the QR service runs them before a sign-in.
    pub async fn bootstrap(&self, session_too: bool) -> Result<(), OmniError> {
        let _turn = self.bootstrapping.lock().await;
        let mut device = self.session().device;
        let now = now_secs();
        let fresh = device.qimei.is_some() && device.qimei_fresh(now);
        if !fresh {
            let (q16, q36) = self.request_qimei(&device).await?;
            device.qimei = Some(q16);
            device.qimei36 = Some(q36);
            device.qimei_saved_at = Some(now);
        }
        if session_too || device.session_uid.is_none() {
            let param = json!({ "uid": device.session_uid.clone().unwrap_or_default(), "vkey": 0, "caller": 0 });
            let payload = json!({ "comm": android_comm(&device, None), "req_0": { "module": "music.getSession.session", "method": "GetSession", "param": param } });
            let data = self.post_android(&payload, None).await?;
            let id = |key: &str| data.pointer(&format!("/session/{key}")).and_then(identifier);
            let (Some(uid), Some(sid)) = (id("uid"), id("sid")) else {
                return Err(OmniError::Response("GetSession response missing uid/sid".to_owned()));
            };
            device.session_uid = Some(uid);
            device.session_sid = Some(sid);
        }
        self.session.lock().unwrap_or_else(PoisonError::into_inner).device = device;
        Ok(())
    }

    /// `requestQimei`: the device report, AES-sealed under a random key that is itself RSA-sealed.
    async fn request_qimei(&self, device: &Device) -> Result<(String, String), OmniError> {
        let key = random_hex(16);
        let nonce = random_hex(16);
        let timestamp = now_secs();
        let sealed_key = STANDARD.encode(rsa_pkcs1(key.as_bytes())?);
        let report = qimei_payload(device, timestamp).to_string();
        let params = STANDARD.encode(cbc_encrypt(key.as_bytes(), key.as_bytes(), report.as_bytes()));
        let extra = format!("{{\"appKey\":\"{QIMEI_APP_KEY}\"}}");
        let sign = md5_hex(&format!("{sealed_key}{params}{}{nonce}{QIMEI_SECRET}{extra}", timestamp * 1000));
        let body = json!({
            "app": 0, "os": 1,
            "qimeiParams": { "key": sealed_key, "params": params, "time": timestamp.to_string(), "nonce": nonce, "sign": sign, "extra": extra },
        });
        let answer: Value = self
            .http
            .post(QIMEI_URL)
            .header("method", "GetQimei")
            .header("service", "trpc.tme_datasvr.qimeiproxy.QimeiProxy")
            .header("appid", "qimei_qq_android")
            .header("sign", md5_hex(&format!("qimei_qq_androidpzAuCmaFAaFaHrdakPjLIEqKrGnSOOvH{timestamp}")))
            .header(reqwest::header::USER_AGENT, "QQMusic")
            .header("timestamp", timestamp.to_string())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let inner = dictionary(answer.get("data").unwrap_or(&Value::Null));
        let data = inner.get("data").cloned().unwrap_or(Value::Null);
        let text = |key: &str| data.get(key).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_owned);
        match (text("q16"), text("q36")) {
            (Some(q16), Some(q36)) => {
                tracing::info!("qq device QIMEI bootstrapped");
                Ok((q16, q36))
            }
            _ => {
                let code = answer.get("code").and_then(Value::as_i64).unwrap_or(-1);
                tracing::warn!(outer = code, inner = ?inner.get("code"), "qq QIMEI bootstrap refused");
                Err(OmniError::Platform(code))
            }
        }
    }

    /// A legacy `c.y.qq.com` CGI with its referer.
    pub async fn legacy(&self, path: &str, params: &[(&str, String)]) -> Result<Value, OmniError> {
        let url = reqwest::Url::parse_with_params(&format!("https://c.y.qq.com{path}"), params)
            .map_err(|err| OmniError::Response(format!("qq address: {err}")))?;
        let mut request = self.http.get(url).header(reqwest::header::REFERER, "https://c.y.qq.com/");
        if let Some(credential) = self.credential() {
            request = request.header(reqwest::header::COOKIE, credential_cookie(&credential));
        }
        let text = request.send().await?.error_for_status()?.text().await?;
        serde_json::from_str(strip_callback(&text)).map_err(|err| OmniError::Response(err.to_string()))
    }

    /// The anonymous `CgiGetVkey` the package asks with its web signature when nobody is signed in.
    pub async fn guest_vkey(&self, filename: &str, songmid: &str) -> Result<Value, OmniError> {
        let data = json!({
            "req_0": { "module": "vkey.GetVkeyServer", "method": "CgiGetVkey", "param": {
                "filename": [filename], "guid": "1429839143", "songmid": [songmid], "songtype": [0], "uin": "0",
                "loginflag": 1, "platform": "20",
            } },
            "loginUin": "0",
            "comm": { "uin": "0", "format": "json", "ct": 24, "cv": 0 },
        });
        let params = [
            ("format", "json".to_owned()),
            ("sign", "zzannc1o6o9b4i971602f3554385022046ab796512b7012".to_owned()),
            ("data", data.to_string()),
        ];
        let url =
            reqwest::Url::parse_with_params(MUSICU, params).map_err(|err| OmniError::Response(err.to_string()))?;
        let answer: Value = self
            .http
            .get(url)
            .header(reqwest::header::REFERER, "https://y.qq.com/portal/player.html")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(answer.pointer("/req_0/data").cloned().unwrap_or(Value::Null))
    }
}

impl Device {
    /// `ensureQimei`'s freshness: both ids, saved less than a day ago.
    fn qimei_fresh(&self, now: u64) -> bool {
        self.qimei36.is_some() && self.qimei_saved_at.is_some_and(|at| now.saturating_sub(at) < QIMEI_TTL_SECS)
    }
}

/// `buildAndroidComm`.
fn android_comm(device: &Device, credential: Option<&Credential>) -> Value {
    let mut comm = json!({
        "ct": 11, "cv": 14090008, "v": 14090008, "chid": CHANNEL_ID, "tmeAppID": "qqmusic",
        "QIMEI": device.qimei.clone().unwrap_or_default(), "QIMEI36": device.qimei36.clone().unwrap_or_default(),
        "OpenUDID": device.open_udid, "udid": device.open_udid, "OpenUDID2": device.open_udid,
        "aid": device.android_id, "os_ver": OS_RELEASE, "phonetype": MODEL, "devicelevel": SDK.to_string(),
        "newdevicelevel": SDK.to_string(), "rom": device.fingerprint,
    });
    if let Some(map) = comm.as_object_mut() {
        if let Some(uid) = &device.session_uid {
            map.insert("uid".to_owned(), Value::from(uid.clone()));
        }
        if let Some(sid) = &device.session_sid {
            map.insert("sid".to_owned(), Value::from(sid.clone()));
        }
        if let Some(credential) = credential {
            map.insert("qq".to_owned(), Value::from(credential.musicid.clone()));
            map.insert("authst".to_owned(), Value::from(credential.musickey.clone()));
            map.insert("tmeLoginType".to_owned(), Value::from(credential.login_type));
        }
    }
    comm
}

/// `credentialCookieHeader`.
fn credential_cookie(credential: &Credential) -> String {
    let id = &credential.musicid;
    let key = &credential.musickey;
    format!("uin={id}; qqmusic_uin={id}; qm_keyst={key}; qqmusic_key={key}")
}

/// A musicu answer's `req_0`, failing on a gateway or business code.
fn unpack(body: Value) -> Result<Value, OmniError> {
    let code = body.get("code").and_then(Value::as_i64).unwrap_or(-1);
    if code != 0 {
        return Err(OmniError::Platform(code));
    }
    let item = body.get("req_0").ok_or_else(|| OmniError::Response("musicu answer without req_0".to_owned()))?;
    match item.get("code").and_then(Value::as_i64) {
        Some(0) => Ok(item.get("data").cloned().unwrap_or(Value::Null)),
        Some(code) => Err(OmniError::Platform(code)),
        None => Err(OmniError::Response("musicu answer without a code".to_owned())),
    }
}

/// `parseDictionary`: an object, or a string holding one.
fn dictionary(value: &Value) -> Value {
    match value {
        Value::String(text) => serde_json::from_str(text).unwrap_or(Value::Null),
        other => other.clone(),
    }
}

/// `identifierOf`: digits or text.
pub fn identifier(value: &Value) -> Option<String> {
    match value {
        Value::Number(number) => Some(number.to_string()),
        Value::String(text) if !text.is_empty() => Some(text.clone()),
        _ => None,
    }
}

/// Old CGIs may wrap JSON in `callback(...)`.
fn strip_callback(text: &str) -> &str {
    let trimmed = text.trim();
    match (trimmed.find('('), trimmed.ends_with(')')) {
        (Some(open), true) if !trimmed.starts_with('{') => &trimmed[open + 1..trimmed.len() - 1],
        _ => trimmed,
    }
}

/// `buildQimeiPayload`.
fn qimei_payload(device: &Device, now: u64) -> Value {
    let uptime = format_datetime(now.saturating_sub(random_below(14401)));
    let reserved = json!({
        "harmony": "0", "clone": "0", "containe": "", "oz": "UhYmelwouA+V2nPWbOvLTgN2/m8jwGB+yUB5v9tysQg=",
        "oo": "Xecjt+9S1+f8Pz2VLSxgpw==", "kelong": "0", "uptimes": uptime, "multiUser": "0", "bod": BRAND,
        "dv": "sagit", "firstLevel": "", "manufact": BRAND, "name": MODEL, "host": "se.infra", "kernel": device.proc_version,
    });
    json!({
        "androidId": device.android_id, "platformId": 1, "appKey": QIMEI_APP_KEY, "appVersion": "14.9.0.8",
        "beaconIdSrc": beacon_id(now), "brand": BRAND, "channelId": CHANNEL_ID, "cid": "", "imei": device.imei,
        "imsi": "", "mac": "", "model": MODEL, "networkType": "unknown", "oaid": "",
        "osVersion": format!("Android {OS_RELEASE},level {SDK}"), "qimei": "", "qimei36": "", "sdkVersion": "1.2.13.6",
        "targetSdkVersion": "33", "audit": "", "userId": "{}", "packageId": "com.tencent.qqmusic", "deviceType": "Phone",
        "sdkName": "", "reserved": reserved.to_string(),
    })
}

/// `randomBeaconId`: forty `kN:value` fields, some dated to the month.
fn beacon_id(now: u64) -> String {
    let month = format!("{}-01", &format_datetime(now)[..7]);
    let first = random_digits(6);
    let second = random_digits(9);
    let dated = [1, 2, 13, 14, 17, 18, 21, 22, 25, 26, 29, 30, 33, 34, 37, 38];
    let mut fields = Vec::with_capacity(40);
    for key in 1..=40 {
        let value = if dated.contains(&key) {
            format!("{month}{first}.{second}")
        } else if key == 3 {
            "0000000000000000".to_owned()
        } else if key == 4 {
            random_hex(16).replace('0', "1")
        } else {
            random_below(10000).to_string()
        };
        fields.push(format!("k{key}:{value}"));
    }
    format!("{};", fields.join(";"))
}

/// `YYYY-MM-DD HH:MM:SS` in UTC.
fn format_datetime(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rest = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}", rest / 3600, rest % 3600 / 60, rest % 60)
}

fn random_imei() -> String {
    let mut digits: Vec<u32> = random_digits(14).chars().filter_map(|c| c.to_digit(10)).collect();
    let sum: u32 = digits
        .iter()
        .enumerate()
        .map(
            |(index, digit)| {
                if index % 2 == 1 { if digit * 2 > 9 { digit * 2 - 9 } else { digit * 2 } } else { *digit }
            },
        )
        .sum();
    digits.push((10 - sum % 10) % 10);
    digits.into_iter().map(|digit| char::from(b'0' + digit as u8)).collect()
}

/// RSAES-PKCS1-v1_5 under the QIMEI bootstrap key.
fn rsa_pkcs1(message: &[u8]) -> Result<Vec<u8>, OmniError> {
    let size = QIMEI_RSA_MODULUS.len() / 2;
    if message.len() + 11 > size {
        return Err(OmniError::Response("QIMEI key too long".to_owned()));
    }
    let mut block = vec![0u8, 2];
    let mut filler = Vec::new();
    while filler.len() < size - 3 - message.len() {
        filler.extend(random_bytes(32).into_iter().filter(|byte| *byte != 0));
    }
    filler.truncate(size - 3 - message.len());
    block.extend(filler);
    block.push(0);
    block.extend_from_slice(message);
    let modulus = BigUint::parse_bytes(QIMEI_RSA_MODULUS.as_bytes(), 16).unwrap_or_default();
    let sealed = BigUint::from_bytes_be(&block).modpow(&BigUint::from(65537u32), &modulus).to_bytes_be();
    let mut out = vec![0u8; size.saturating_sub(sealed.len())];
    out.extend(sealed);
    Ok(out)
}

/// AES-128-CBC with PKCS#7.
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

pub fn md5_hex(text: &str) -> String {
    Md5::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn random_hex(len: usize) -> String {
    random_bytes(len.div_ceil(2)).iter().map(|byte| format!("{byte:02x}")).collect::<String>()[..len].to_owned()
}

fn random_digits(len: usize) -> String {
    random_bytes(len).into_iter().map(|byte| char::from(b'0' + byte % 10)).collect()
}

fn random_below(limit: u64) -> u64 {
    let bytes = random_bytes(8);
    let mut word = [0u8; 8];
    word.copy_from_slice(&bytes);
    u64::from_be_bytes(word) % limit.max(1)
}

/// Random bytes from the standard library's per-process random hasher keys and the clock.
pub fn random_bytes(len: usize) -> Vec<u8> {
    use std::hash::{BuildHasher, Hasher};
    let mut bytes = Vec::with_capacity(len + 8);
    let mut round = 0u64;
    while bytes.len() < len {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(round);
        hasher.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos());
        bytes.extend_from_slice(&hasher.finish().to_be_bytes());
        round += 1;
    }
    bytes.truncate(len);
    bytes
}

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

pub fn now_millis() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}
