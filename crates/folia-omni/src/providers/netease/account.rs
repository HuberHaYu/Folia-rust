// NetEase QR sign-in (unikey → code → poll 800–803), the signed-in account, and logout.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value, json};

use super::eapi::{self, Client, Session};
use crate::{Account, LoginCode, LoginState, OmniError, QrContent};

/// `login_qr_key`: a key for a new code (`type: 3`, the desktop client's).
pub async fn login_code(client: &Client) -> Result<LoginCode, OmniError> {
    let answer = client.post("/api/login/qrcode/unikey", object(json!({ "type": 3 }))).await?;
    eapi::check(&answer.body)?;
    let key = answer
        .body
        .get("unikey")
        .and_then(Value::as_str)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| OmniError::Response("no QR key".to_owned()))?
        .to_owned();
    let device = client.session().device_id;
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    let url = format!("https://music.163.com/login?codekey={key}&chainId=v1_{device}_web_login_{millis}");
    Ok(LoginCode { key, content: QrContent::Url(url) })
}

/// `login_qr_check`. On 803 the answer's cookies become the session.
pub async fn check_login(client: &Client, key: &str) -> Result<LoginState, OmniError> {
    let answer = client.post("/api/login/qrcode/client/login", object(json!({ "key": key, "type": 3 }))).await?;
    let code = eapi::code_of(&answer.body);
    Ok(match code {
        800 => LoginState::Expired,
        801 => LoginState::Waiting,
        802 => LoginState::Scanned,
        803 => {
            let cookie = |name: &str| {
                answer
                    .cookies
                    .iter()
                    .find(|(key, value)| key == name && !value.is_empty())
                    .map(|(_, value)| value.clone())
            };
            let music_u = cookie("MUSIC_U").or_else(|| cookie_field(&answer.body, "MUSIC_U"));
            let csrf = cookie("__csrf").or_else(|| cookie_field(&answer.body, "__csrf"));
            if music_u.is_none() {
                return Ok(LoginState::Failed { code, message: Some("confirmed without a session".to_owned()) });
            }
            let device_id = client.session().device_id;
            client.set_session(Session { device_id, music_u, csrf });
            tracing::info!("netease sign-in confirmed");
            LoginState::Confirmed
        }
        _ => {
            LoginState::Failed { code, message: answer.body.get("message").and_then(Value::as_str).map(str::to_owned) }
        }
    })
}

/// `user_account`: the profile of the session, `None` when it has none.
pub async fn account(client: &Client) -> Result<Option<Account>, OmniError> {
    if !client.session().signed_in() {
        return Ok(None);
    }
    let answer = client.post("/api/nuser/account/get", Map::new()).await?;
    match eapi::check(&answer.body) {
        Ok(()) => {}
        Err(OmniError::SignedOut) => return Ok(None),
        Err(err) => return Err(err),
    }
    let Some(profile) = answer.body.get("profile").filter(|profile| profile.is_object()) else { return Ok(None) };
    let id = profile
        .get("userId")
        .or_else(|| answer.body.pointer("/account/id"))
        .and_then(Value::as_u64)
        .ok_or_else(|| OmniError::Response("account without an id".to_owned()))?;
    let nickname = profile.get("nickname").and_then(Value::as_str).unwrap_or_default().to_owned();
    let avatar_url =
        profile.get("avatarUrl").and_then(Value::as_str).filter(|url| !url.is_empty()).map(|url| {
            url.strip_prefix("http://").map(|rest| format!("https://{rest}")).unwrap_or_else(|| url.to_owned())
        });
    Ok(Some(Account { id: id.to_string(), nickname, avatar_url }))
}

/// `logout`: the request carries the session as it is, then the session goes whatever NetEase answers.
pub async fn logout(client: &Client) -> Result<(), OmniError> {
    let told = client.post("/api/logout", Map::new()).await;
    let device_id = client.session().device_id;
    client.set_session(Session { device_id, music_u: None, csrf: None });
    told.map(|_| ())
}

/// A cookie the answer names in its `cookie` field (`name=value; ...`), as some gateways send it.
fn cookie_field(body: &Value, name: &str) -> Option<String> {
    body.get("cookie")?
        .as_str()?
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, value)| *key == name && !value.is_empty())
        .map(|(_, value)| value.to_owned())
}

pub(super) fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}
