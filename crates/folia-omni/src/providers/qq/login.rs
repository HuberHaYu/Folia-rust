// QQ sign-in by the QQ Connect web QR (`ptlogin.ts` of qq-music-api 3.3.3): `ptqrshow`, `ptqrlogin`, then
// `check_sig` → `login_jump` → `authorize` → `QQConnectLogin.QQLogin` for the QQ Music credential.

use serde_json::{Value, json};

use super::musicu::{self, Client, Credential, MUSICU};
use crate::{LoginCode, LoginState, OmniError, QrContent};

const PTLOGIN: &str = "https://xui.ptlogin2.qq.com";
const LOGIN_JUMP: &str = "https://graph.qq.com/oauth2.0/login_jump";
const AUTHORIZE: &str = "https://graph.qq.com/oauth2.0/authorize";
const REDIRECT: &str = "https://y.qq.com/portal/wx_redirect.html";
/// QQ Music's web identity at QQ Connect: public values from the official page.
const APP_ID: &str = "716027609";
const CLIENT_ID: &str = "100497308";
const SCOPE: &str = "get_user_info,get_app_friends";

/// `hash33`: `ptqrtoken` starts from 0, `g_tk` from 5381.
fn hash33(value: &str, initial: i64) -> i64 {
    let mut result = initial;
    for ch in value.encode_utf16() {
        result = (result + ((result << 5) & 0xffff_ffff) + i64::from(ch)) & 0xffff_ffff;
    }
    result & 0x7fff_ffff
}

/// `createPtloginQr`: the QR picture and its `qrsig`, the key every poll sends back.
pub async fn login_code(client: &Client, plain: &reqwest::Client) -> Result<LoginCode, OmniError> {
    client.bootstrap(true).await?;
    let t = format!("0.{}", musicu::random_bytes(8).iter().map(|b| (b % 10).to_string()).collect::<String>());
    let params = [
        ("appid", APP_ID),
        ("e", "2"),
        ("l", "M"),
        ("s", "3"),
        ("d", "72"),
        ("v", "4"),
        ("t", t.as_str()),
        ("daid", "383"),
        ("pt_3rd_aid", CLIENT_ID),
        ("u1", LOGIN_JUMP),
    ];
    let url = reqwest::Url::parse_with_params(&format!("{PTLOGIN}/ssl/ptqrshow"), params)
        .map_err(|err| OmniError::Response(err.to_string()))?;
    let response = plain.get(url).send().await?.error_for_status()?;
    let key = cookie_of(&response, "qrsig", "ptlogin2.qq.com")
        .ok_or_else(|| OmniError::Response("Ptlogin QR has no qrsig".to_owned()))?;
    let png = response.bytes().await?.to_vec();
    if !png.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]) {
        return Err(OmniError::Response("Ptlogin QR image is not a PNG".to_owned()));
    }
    Ok(LoginCode { key, content: QrContent::Png(png) })
}

/// `pollPtloginQr`: 66 waiting, 67 scanned, 65 expired, 0 confirmed; a confirmation is exchanged at once.
pub async fn check_login(client: &Client, plain: &reqwest::Client, qrsig: &str) -> Result<LoginState, OmniError> {
    let token = hash33(qrsig, 0).to_string();
    let params = [
        ("u1", LOGIN_JUMP),
        ("from_ui", "1"),
        ("aid", APP_ID),
        ("daid", "383"),
        ("pt_3rd_aid", CLIENT_ID),
        ("ptqrtoken", token.as_str()),
    ];
    let url = reqwest::Url::parse_with_params(&format!("{PTLOGIN}/ssl/ptqrlogin"), params)
        .map_err(|err| OmniError::Response(err.to_string()))?;
    let text = plain
        .get(url)
        .header(reqwest::header::COOKIE, format!("qrsig={qrsig}"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    let args =
        ptui_arguments(&text).ok_or_else(|| OmniError::Response("Ptlogin poll response is malformed".to_owned()))?;
    let code: i64 = args.first().and_then(|code| code.parse().ok()).unwrap_or(-1);
    Ok(match code {
        66 => LoginState::Waiting,
        67 => LoginState::Scanned,
        65 => LoginState::Expired,
        0 => {
            let check_sig = args.get(2).cloned().unwrap_or_default();
            let credential = exchange(plain, &check_sig).await?;
            client.set_credential(Some(credential));
            tracing::info!("qq sign-in confirmed");
            LoginState::Confirmed
        }
        code => LoginState::Failed { code, message: args.get(4).cloned() },
    })
}

/// `exchangePtloginCredential`: every hop checked against the host and path it must have.
async fn exchange(plain: &reqwest::Client, check_sig: &str) -> Result<Credential, OmniError> {
    let check_sig = trusted(check_sig, "ssl.ptlogin2.graph.qq.com", "/check_sig")?;
    let checked = plain.get(check_sig).send().await?;
    if checked.status() != reqwest::StatusCode::FOUND {
        return Err(OmniError::Response("Ptlogin check_sig did not redirect".to_owned()));
    }
    let location = checked.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()).unwrap_or_default();
    let jump = trusted(location, "graph.qq.com", "/oauth2.0/login_jump")?;
    let mut graph = Vec::new();
    for name in ["p_uin", "p_skey", "pt_oauth_token"] {
        let value = cookie_of(&checked, name, "graph.qq.com")
            .ok_or_else(|| OmniError::Response("Ptlogin graph cookies are missing".to_owned()))?;
        graph.push((name, value));
    }
    let cookie = graph.iter().map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join("; ");
    let jumped = plain.get(jump).send().await?;
    if jumped.status() != reqwest::StatusCode::OK {
        return Err(OmniError::Response("Ptlogin login_jump failed".to_owned()));
    }
    let state = musicu::random_hex(32);
    let redirect = reqwest::Url::parse_with_params(REDIRECT, [("login_type", "1"), ("surl", "https://y.qq.com/")])
        .map_err(|err| OmniError::Response(err.to_string()))?;
    let g_tk = hash33(&graph[1].1, 5381).to_string();
    let auth_time = musicu::now_millis().to_string();
    let ui = uuid_v4();
    let form = [
        ("response_type", "code"),
        ("client_id", CLIENT_ID),
        ("redirect_uri", redirect.as_str()),
        ("scope", SCOPE),
        ("state", state.as_str()),
        ("switch", ""),
        ("from_ptlogin", "1"),
        ("src", "1"),
        ("update_auth", "1"),
        ("openapi", "1010_1030"),
        ("g_tk", g_tk.as_str()),
        ("auth_time", auth_time.as_str()),
        ("ui", ui.as_str()),
    ];
    let authorized = plain.post(AUTHORIZE).header(reqwest::header::COOKIE, cookie).form(&form).send().await?;
    if authorized.status() != reqwest::StatusCode::FOUND {
        return Err(OmniError::Response("Ptlogin authorize did not redirect".to_owned()));
    }
    let location =
        authorized.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()).unwrap_or_default();
    let back = trusted(location, "y.qq.com", "/portal/wx_redirect.html")?;
    let all = |name: &str| {
        back.query_pairs().filter(|(key, _)| key == name).map(|(_, value)| value.into_owned()).collect::<Vec<_>>()
    };
    let (codes, states) = (all("code"), all("state"));
    let code = match codes.as_slice() {
        [code] if code.len() == 32 && code.bytes().all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(&b)) => {
            code.clone()
        }
        _ => return Err(OmniError::Response("Ptlogin authorization code is missing".to_owned())),
    };
    if states.as_slice() != [state] {
        return Err(OmniError::Response("Ptlogin state does not match".to_owned()));
    }
    let body = json!({
        "comm": { "g_tk": 5381, "platform": "yqq", "ct": 24, "cv": 0 },
        "req": { "module": "QQConnectLogin.LoginServer", "method": "QQLogin", "param": { "code": code } },
    });
    let answer: Value = plain
        .post(MUSICU)
        .header(reqwest::header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(body.to_string())
        .send()
        .await?
        .json()
        .await?;
    let global = answer.get("code").and_then(Value::as_i64);
    let request = answer.pointer("/req/code").and_then(Value::as_i64);
    if global != Some(0) || request != Some(0) {
        return Err(OmniError::Platform(request.or(global).unwrap_or(-1)));
    }
    credential_of(answer.pointer("/req/data").unwrap_or(&Value::Null))
}

/// `credentialOf`: a web credential (`loginType` 2) with a key and a positive music id.
fn credential_of(data: &Value) -> Result<Credential, OmniError> {
    let musicid = data
        .get("str_musicid")
        .and_then(musicu::identifier)
        .filter(|id| id != "0")
        .or_else(|| data.get("musicid").and_then(musicu::identifier))
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && id != "0");
    let musickey = data.get("musickey").and_then(Value::as_str).filter(|key| !key.is_empty());
    let login_type = data.get("loginType").and_then(Value::as_i64);
    match (musicid, musickey, login_type) {
        (Some(musicid), Some(musickey), Some(2)) => Ok(Credential {
            musicid,
            musickey: musickey.to_owned(),
            login_type: 2,
            encrypt_uin: data.get("encryptUin").and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_owned),
        }),
        _ => Err(OmniError::Response("Ptlogin credential is invalid".to_owned())),
    }
}

/// `trustedUrl`: https, the host and path given, no credentials, port or fragment.
fn trusted(raw: &str, host: &str, path: &str) -> Result<reqwest::Url, OmniError> {
    let url =
        reqwest::Url::parse(raw).map_err(|_| OmniError::Response("Ptlogin redirect is not allowed".to_owned()))?;
    let ok = url.scheme() == "https"
        && url.host_str() == Some(host)
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == path
        && url.fragment().is_none();
    if ok { Ok(url) } else { Err(OmniError::Response("Ptlogin redirect is not allowed".to_owned())) }
}

/// `cookieOf`: the last value of `name` set for exactly `domain`, or nothing once it is cleared.
fn cookie_of(response: &reqwest::Response, name: &str, domain: &str) -> Option<String> {
    let mut found = None;
    for header in response.headers().get_all(reqwest::header::SET_COOKIE).iter().filter_map(|v| v.to_str().ok()) {
        let mut parts = header.split(';').map(str::trim);
        let Some((key, value)) = parts.next().and_then(|first| first.split_once('=')) else { continue };
        if key != name {
            continue;
        }
        let attributes: Vec<&str> = parts.collect();
        let declared = attributes
            .iter()
            .find(|part| part.to_ascii_lowercase().starts_with("domain="))
            .map(|part| part[7..].to_ascii_lowercase().trim_start_matches('.').to_owned());
        if declared.as_deref() != Some(domain) {
            continue;
        }
        let cleared = attributes.iter().any(|part| part.eq_ignore_ascii_case("max-age=0"));
        let valid = !value.is_empty()
            && value.len() <= 512
            && value.bytes().all(|b| (0x21..=0x7e).contains(&b) && b != b'"' && b != b',' && b != b';' && b != b'\\');
        found = (!cleared && valid).then(|| value.to_owned());
    }
    found
}

/// `ptuiArgumentsOf`: the quoted arguments of `ptuiCB('a','b',…)`.
fn ptui_arguments(body: &str) -> Option<Vec<String>> {
    let inner = body.trim().strip_prefix("ptuiCB(")?;
    let inner = inner.trim_end().trim_end_matches(';').strip_suffix(')')?;
    let mut values = Vec::new();
    let mut chars = inner.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace() || *c == ',') {
            chars.next();
        }
        match chars.next() {
            None => break,
            Some('\'') => {}
            Some(_) => return None,
        }
        let mut value = String::new();
        loop {
            match chars.next()? {
                '\'' => break,
                '\\' => match chars.next()? {
                    'n' => value.push('\n'),
                    'r' => value.push('\r'),
                    't' => value.push('\t'),
                    'x' => value.push(char::from_u32(u32::from_str_radix(&take(&mut chars, 2)?, 16).ok()?)?),
                    'u' => value.push(char::from_u32(u32::from_str_radix(&take(&mut chars, 4)?, 16).ok()?)?),
                    other => value.push(other),
                },
                other => value.push(other),
            }
        }
        values.push(value);
    }
    let first_ok = values
        .first()
        .is_some_and(|code| !code.is_empty() && code.len() <= 9 && code.bytes().all(|b| b.is_ascii_digit()));
    ((6..=7).contains(&values.len()) && first_ok).then_some(values)
}

fn take(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, count: usize) -> Option<String> {
    let text: String = chars.take(count).collect();
    (text.len() == count).then_some(text)
}

/// `crypto.randomUUID`.
fn uuid_v4() -> String {
    let mut bytes = musicu::random_bytes(16);
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}
