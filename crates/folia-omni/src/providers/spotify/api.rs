// What is asked of Spotify as the account: pathfinder's persisted queries and spclient (`spotifyApiBridge.cjs`).

use std::sync::{Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::OmniError;

const QUERY_URL: &str = "https://api-partner.spotify.com/pathfinder/v2/query";
pub const SPCLIENT_URL: &str = "https://spclient.wg.spotify.com";
/// A token is renewed this long before Spotify would refuse it (`RENEW_AHEAD_MS`).
const RENEW_AHEAD_MS: u64 = 60 * 1000;

/// `QUERIES` and `CHANGES`: each persisted query with the hash Spotify knows it by (read 2026-10-08).
pub const QUERIES: [(&str, &str); 19] = [
    ("home", "3357ffed7961629ba92b4e0a41514e4d5004a14355c964c23ce442205c9e44a1"),
    ("homeSection", "d62af2714f2623c923cc9eeca4b9545b4363abaa9188a9e94e2b63b823419a2c"),
    ("browsePage", "f5c4e6d668f5716464a231c1cc8b22c1cbf6ad68b09929fd7de813a30581298b"),
    ("queryWhatsNewFeed", "3b53dede3c6054e8b7c962dd280eb6761c5d1c82b06b039f4110d76a62b4966b"),
    ("profileAttributes", "53bcb064f6cd18c23f752bc324a791194d20df612d8e1239c735144ab0399ced"),
    ("libraryV3", "390c78e5b951029bad359785e69b07b536a509c581cbcd0aded5e5067f187455"),
    ("fetchLibraryTracks", "087278b20b743578a6262c2b0b4bcd20d879c503cc359a2285baf083ef944240"),
    ("fetchPlaylist", "cd2275433b29f7316176e7b5b5e098ae7744724e1a52d63549c76636b3257749"),
    ("getAlbum", "b9bfabef66ed756e5e13f68a942deb60bd4125ec1f1be8cc42769dc0259b4b10"),
    ("queryArtistOverview", "7f86ff63e38c24973a2842b672abe44c910c1973978dc8a4a0cb648edef34527"),
    ("queryArtistDiscographyAll", "5e07d323febb57b4a56a42abbf781490e58764aa45feb6e3dc0591564fc56599"),
    ("searchTracks", "bc1ca2fcd0ba1013a0fc88e6cc4f190af501851e3dafd3e1ef85840297694428"),
    ("getTrack", "612585ae06ba435ad26369870deaae23b5c8800a256cd8a57e08eddc25a37294"),
    ("areEntitiesInLibrary", "134337999233cc6fdd6b1e6dbf94841409f04a946c5c7b744b09ba0dfe5a85ed"),
    ("addToLibrary", "896ebcb47815681340860d121cb5d494e157e2a78d3950385cd54e0393c67148"),
    ("removeFromLibrary", "896ebcb47815681340860d121cb5d494e157e2a78d3950385cd54e0393c67148"),
    ("addItemsToRootlist", "bd9c5cae1ee80ebca05d7ed12fd394216f49a20ce72d9dc762868df0f14522ea"),
    ("removeItemsFromRootlist", "3422f1866532820a1d8d0560f88542dbdbd35d0377ee5ecd49593590f5f1e86b"),
    ("addToPlaylist", "47b2a1234b17748d332dd0431534f22450e9ecbb3d5ddcdacbd83368636a0990"),
];

/// The tokens of the sign-in. Never logged.
#[derive(Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Milliseconds since the epoch.
    pub expires_at: u64,
}

/// The account as `readAccount` reads it; `id` is `publicAccountId`, not Spotify's user name.
#[derive(Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub nickname: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub premium: Option<bool>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub tokens: Tokens,
    pub user: User,
}

pub struct Client {
    pub http: reqwest::Client,
    session: Mutex<Option<Session>>,
    renewing: tokio::sync::Mutex<()>,
}

impl Client {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http, session: Mutex::new(None), renewing: tokio::sync::Mutex::new(()) }
    }

    pub fn session(&self) -> Option<Session> {
        self.session.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    pub fn set_session(&self, session: Option<Session>) {
        *self.session.lock().unwrap_or_else(PoisonError::into_inner) = session;
    }

    /// `accessToken`: the stored one, renewed once for all who find it old.
    pub async fn access_token(&self, fresh: bool) -> Result<String, OmniError> {
        let session = self.session().ok_or(OmniError::SignedOut)?;
        if !fresh && session.tokens.expires_at > now_millis() + RENEW_AHEAD_MS {
            return Ok(session.tokens.access_token);
        }
        let _turn = self.renewing.lock().await;
        let current = self.session().ok_or(OmniError::SignedOut)?;
        if current.tokens.access_token != session.tokens.access_token {
            return Ok(current.tokens.access_token);
        }
        let refresh = current.tokens.refresh_token.clone();
        match super::signin::tokens(
            &self.http,
            &[("grant_type", "refresh_token"), ("refresh_token", &refresh)],
            Some(&refresh),
        )
        .await
        {
            Ok(tokens) => {
                let token = tokens.access_token.clone();
                if let Some(kept) = self.session.lock().unwrap_or_else(PoisonError::into_inner).as_mut() {
                    kept.tokens = tokens;
                }
                tracing::info!("spotify token renewed");
                Ok(token)
            }
            Err(OmniError::SignedOut) => {
                self.set_session(None);
                tracing::warn!("spotify no longer accepts the kept sign-in");
                Err(OmniError::SignedOut)
            }
            Err(err) => Err(err),
        }
    }

    /// `call`: one request as the account; a 401 is asked once more with a renewed token; 404 reads as `None`.
    pub async fn call(
        &self,
        url: &str,
        body: Option<Value>,
        token: Option<&str>,
        extra: &[(&str, &str)],
    ) -> Result<Option<Value>, OmniError> {
        let send = |bearer: String| {
            let mut request = match &body {
                Some(body) => {
                    self.http.post(url).header(reqwest::header::CONTENT_TYPE, "application/json").body(body.to_string())
                }
                None => self.http.get(url),
            };
            request = request
                .header(reqwest::header::ACCEPT, "application/json")
                .header(reqwest::header::ACCEPT_LANGUAGE, "zh-CN")
                .bearer_auth(bearer);
            for (name, value) in extra {
                request = request.header(*name, *value);
            }
            request.send()
        };
        let bearer = match token {
            Some(token) => token.to_owned(),
            None => self.access_token(false).await?,
        };
        let mut response = send(bearer).await?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED && token.is_none() {
            response = send(self.access_token(true).await?).await?;
        }
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(OmniError::SignedOut);
        }
        Ok(Some(response.error_for_status()?.json().await?))
    }

    /// `query`: a persisted query by name; an answer without data fails, one answered in part is logged.
    pub async fn query(&self, name: &str, variables: Value, token: Option<&str>) -> Result<Value, OmniError> {
        let hash = QUERIES
            .iter()
            .find(|(query, _)| *query == name)
            .map(|(_, hash)| *hash)
            .ok_or_else(|| OmniError::Response(format!("unknown Spotify query {name}")))?;
        let body = json!({ "variables": variables, "operationName": name, "extensions": { "persistedQuery": { "version": 1, "sha256Hash": hash } } });
        let answer = self.call(QUERY_URL, Some(body), token, &[]).await?.unwrap_or(Value::Null);
        let said: Vec<&str> = answer
            .get("errors")
            .and_then(Value::as_array)
            .map(|errors| errors.iter().filter_map(|e| e.get("message").and_then(Value::as_str)).collect())
            .unwrap_or_default();
        match answer.get("data").filter(|data| !data.is_null()) {
            Some(data) => {
                if !said.is_empty() {
                    tracing::warn!(query = name, said = %said.join(" | "), "spotify answered in part");
                }
                Ok(data.clone())
            }
            None => {
                tracing::warn!(query = name, said = %said.join(" | "), "spotify query failed");
                Err(OmniError::Response(format!("Spotify did not answer the query {name}")))
            }
        }
    }

    /// `change`: a persisted mutation; every result must be there.
    pub async fn change(&self, name: &str, variables: Value) -> Result<(), OmniError> {
        let data = self.query(name, variables, None).await?;
        let results: Vec<&Value> = data.as_object().map(|map| map.values().collect()).unwrap_or_default();
        if results.is_empty() || results.iter().any(|result| result.is_null()) {
            return Err(OmniError::Response(format!("Spotify refused the change {name}")));
        }
        Ok(())
    }
}

pub fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
