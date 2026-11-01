// KuGou QR sign-in (`login_qr_key` → code → `login_qr_check` 0–4) and the signed-in account (`user_detail`).

use serde::Serialize;
use serde_json::{Value, json};

use super::gateway::{self, APPID, Client, Request, SRCAPPID, Sign};
use super::{cover_of, data_of, text_of};
use crate::{Account, LoginCode, LoginState, OmniError};

/// `login_qr_key` with the default type: the lite client's app id 1001.
pub async fn login_code(client: &Client) -> Result<LoginCode, OmniError> {
    let answer = client
        .call(|| {
            let mut request = Request::get("/v2/qrcode")
                .base("https://login-user.kugou.com")
                .param("appid", 1001)
                .param("type", 1)
                .param("plat", 4)
                .param("qrcode_txt", format!("https://h5.kugou.com/apps/loginQRCode/html/index.html?appid={APPID}&"))
                .param("srcappid", SRCAPPID);
            request.sign = Sign::Web;
            request
        })
        .await?;
    let key = text_of(data_of(&answer), &["qrcode", "qrkey", "key", "ticket"])
        .ok_or_else(|| OmniError::Response("no QR key".to_owned()))?;
    // `login_qr_create`.
    let url = format!("https://h5.kugou.com/apps/loginQRCode/html/index.html?qrcode={key}");
    Ok(LoginCode { key, url })
}

/// `login_qr_check`: 0 expired, 1 waiting, 2 and 3 scanned, 4 confirmed with the token and the user id.
pub async fn check_login(client: &Client, key: &str) -> Result<LoginState, OmniError> {
    let answer = client
        .call(|| {
            let mut request = Request::get("/v2/get_userinfo_qrcode")
                .base("https://login-user.kugou.com")
                .param("plat", 4)
                .param("appid", APPID)
                .param("srcappid", SRCAPPID)
                .param("qrcode", key);
            request.sign = Sign::Web;
            request
        })
        .await?;
    let data = data_of(&answer);
    let status = data.get("status").or_else(|| answer.get("status")).and_then(Value::as_i64).unwrap_or(-1);
    let token = text_of(data, &["token"]);
    Ok(match status {
        0 => LoginState::Expired,
        1 => LoginState::Waiting,
        2 | 3 => LoginState::Scanned,
        _ if status == 4 || token.is_some() => {
            let Some(userid) = text_of(data, &["userid", "user_id"]) else {
                return Ok(LoginState::Failed { code: status, message: Some("confirmed without a user".to_owned()) });
            };
            client.update(|session| {
                session.token = token;
                session.userid = Some(userid);
                if let Some(dfid) = text_of(data, &["dfid"]) {
                    session.dfid = Some(dfid);
                }
            });
            tracing::info!("kugou sign-in confirmed");
            LoginState::Confirmed
        }
        code => LoginState::Failed { code, message: text_of(data, &["message", "msg", "error"]) },
    })
}

/// `user_detail`: the profile behind the token, `None` when the session has none or KuGou no longer takes it.
pub async fn account(client: &Client) -> Result<Option<Account>, OmniError> {
    let session = client.session();
    if !session.signed_in() {
        return Ok(None);
    }
    let answer = client
        .call(|| {
            let session = client.session();
            let visit_time = gateway::now().as_secs();
            #[derive(Serialize)]
            struct Sealed<'a> {
                token: &'a str,
                clienttime: u64,
            }
            let sealed = serde_json::to_string(&Sealed {
                token: session.token.as_deref().unwrap_or_default(),
                clienttime: visit_time,
            })
            .unwrap_or_default();
            let p = gateway::rsa_zero_padded(&sealed).unwrap_or_default().to_uppercase();
            let userid: u64 = session.userid.as_deref().and_then(|id| id.parse().ok()).unwrap_or(0);
            let body = json!({ "visit_time": visit_time, "usertype": 1, "p": p, "userid": userid });
            Request::post("/v3/get_my_info", body).param("plat", 1).router("usercenter.kugou.com")
        })
        .await;
    let answer = match answer {
        Ok(answer) => answer,
        Err(OmniError::SignedOut) => return Ok(None),
        Err(err) => return Err(err),
    };
    let data = data_of(&answer);
    let profile = ["user_info", "userinfo", "profile"].iter().find_map(|key| data.get(*key)).unwrap_or(data);
    let nickname = text_of(profile, &["nickname", "nick_name", "username", "name"]).unwrap_or_default();
    if nickname.is_empty() {
        return Ok(None);
    }
    let avatar_url = cover_of(profile).or_else(|| text_of(profile, &["avatar", "pic"]));
    // The bridge answers the session's user id in place of the profile's.
    let id =
        session.userid.clone().or_else(|| text_of(profile, &["userid", "user_id", "id", "uid"])).unwrap_or_default();
    Ok(Some(Account { id, nickname, avatar_url }))
}
