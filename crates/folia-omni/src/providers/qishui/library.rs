// Qishui's sign-in, catalogue, account lists and feed queues (the package's `auth`, `catalog`, `feed`, `playback`).

use std::sync::{Mutex, PoisonError};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use folia_core::{Lyrics, Track};
use folia_lyrics::Format;
use serde_json::{Value, json};

use super::client::{self, Body, CLIENT_KEY, Client, Credential, PREFIX};
use super::{collection, image, text, track, tracks};
use crate::{
    Account, AudioSource, Collection, CollectionDetail, CollectionKind, ExploreEntry, ExploreSection, LoginCode,
    LoginState, OmniError, Page, QrContent, RadioFeed, Shelf,
};

const RECENT_ID: &str = "recent-played";
const SCENE_PREFIX: &str = "scene-";
const MAX_PAGES: usize = 500;
/// Plaintext quality names and their rank; `high` is asked for.
const LEVELS: [(&str, u8); 5] = [("medium", 0), ("higher", 1), ("highest", 1), ("lossless", 2), ("hi_res", 3)];
const WANTED_RANK: u8 = 1;

/// `qrParams`: the car client's Douyin OAuth identity.
fn qr_params() -> Vec<(&'static str, String)> {
    vec![
        ("aid", "1128".to_owned()),
        ("client_key", CLIENT_KEY.to_owned()),
        ("scope", "user_info,luna.openapi.platform.interaction,luna.openapi.platform.me_tab,luna.commerce.openapi_platform.vip_info".to_owned()),
        ("state", String::new()),
        ("next", String::new()),
        ("jump_type", "native".to_owned()),
        ("optional_scope_check", String::new()),
        ("optional_scope_uncheck", String::new()),
        ("customize_params", "{\"source\":\"pc_auth\",\"not_skip_confirm\":\"true\"}".to_owned()),
        ("source_from", "native".to_owned()),
        ("device_platform", "android".to_owned()),
        ("app_identity", md5_hex("com.luna.music.car")),
        ("signature", "cf9024ea72d9c71813238b13ea107a3d".to_owned()),
    ]
}

fn md5_hex(text: &str) -> String {
    use md5::{Digest, Md5};
    Md5::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

fn url(base: &str, params: &[(&str, String)]) -> Result<reqwest::Url, OmniError> {
    reqwest::Url::parse_with_params(base, params).map_err(|err| OmniError::Response(format!("qishui address: {err}")))
}

/// `start`: Douyin's QR for the car client; its token is the key every check sends back.
pub async fn login_code(client: &Client) -> Result<LoginCode, OmniError> {
    let (data, _) =
        client.http(url("https://open.douyin.com/oauth/get_qrcode/", &qr_params())?, None, Vec::new()).await?;
    let data = data.get("data").unwrap_or(&Value::Null);
    let (Some(key), Some(png)) = (text(data, "token"), text(data, "qrcode")) else {
        return Err(OmniError::Response("QR code is missing".to_owned()));
    };
    let png = STANDARD.decode(png).map_err(|err| OmniError::Response(format!("qishui QR: {err}")))?;
    Ok(LoginCode { key, content: QrContent::Png(png) })
}

/// `check`: new, scanned, confirmed (the code becomes an access code, then a guarded ticket), expired, refused.
pub async fn check_login(client: &Client, token: &str) -> Result<LoginState, OmniError> {
    let mut params = qr_params();
    params.push(("token", token.to_owned()));
    params.push(("timestamp", client::now_millis().to_string()));
    let (data, _) = client.http(url("https://open.douyin.com/oauth/check_qrcode/", &params)?, None, Vec::new()).await?;
    let data = data.get("data").unwrap_or(&Value::Null);
    match data.get("status").and_then(Value::as_str) {
        Some("new") => return Ok(LoginState::Waiting),
        Some("scanned") => return Ok(LoginState::Scanned),
        Some("expired") => return Ok(LoginState::Expired),
        Some("refused") => return Ok(LoginState::Failed { code: 0, message: Some("refused".to_owned()) }),
        Some("confirmed") => {}
        _ => return Err(OmniError::Response("Invalid QR status".to_owned())),
    }
    let code = text(data, "code")
        .or_else(|| text(data, "auth_code"))
        .ok_or_else(|| OmniError::Response("QR authorization code is missing".to_owned()))?;
    let access_url =
        client.query(&format!("https://open.qishui.com{PREFIX}/access/code/get/"), &[("auth_code", code)])?;
    let (access, _) = client.http(access_url, None, Vec::new()).await?;
    let access = access.get("data").filter(|inner| inner.is_object()).cloned().unwrap_or(access);
    let (guard_key, csr) = client::create_guard(&client.rng)?;
    let mut credential = ticket(client, &access, guard_key, ("bd-ticket-guard-client-csr", csr)).await?;
    let user = read_user(client, &credential).await?;
    credential.user_id = Some(user.id.clone());
    credential.nickname = Some(user.nickname.clone());
    credential.avatar_url = user.avatar_url.clone();
    client.set_credential(Some(credential));
    tracing::info!("qishui sign-in confirmed");
    Ok(LoginState::Confirmed)
}

/// `ticket`: the access code exchanged at the passport for a ticket bound to the guard key.
async fn ticket(
    client: &Client,
    access: &Value,
    guard_key: String,
    guard: (&'static str, String),
) -> Result<Credential, OmniError> {
    let access_code =
        text(access, "access_code").ok_or_else(|| OmniError::Response("Access code is missing".to_owned()))?;
    let open_id = text(access, "open_id").or_else(|| text(access, "open_uid")).unwrap_or_default();
    let passport = url(
        "https://open-client.douyin.com/passport/open/get_access_ticket/",
        &[("aid", "1128".to_owned()), ("client_key", CLIENT_KEY.to_owned()), ("device_platform", "android".to_owned())],
    )?;
    let form = Body::Form(vec![("access_code", access_code.clone()), ("open_id", open_id.clone())]);
    let (_, headers) =
        client.http(passport, Some(form), vec![("bd-ticket-guard-version", "1".to_owned()), guard]).await?;
    let server = headers.get("bd-ticket-guard-server-data").and_then(|v| v.to_str().ok()).unwrap_or_default();
    let (ticket, guard_ts_sign, guard_cert) = client::read_guard(server)?;
    Ok(Credential {
        ticket,
        guard_key,
        guard_cert,
        guard_ts_sign,
        access_code,
        open_id,
        expires_at: client::expiry(access.get("expires_time")),
        user_id: None,
        nickname: None,
        avatar_url: None,
    })
}

/// `getUser`: `/me/info/` under the credential.
async fn read_user(client: &Client, credential: &Credential) -> Result<Account, OmniError> {
    let data = client.platform_as("/me/info/", credential).await?;
    let user = data.get("my_info").unwrap_or(&Value::Null);
    let id = text(user, "id").filter(|id| id.len() <= 512 && !id.chars().any(char::is_whitespace));
    let id = id.ok_or_else(|| OmniError::Response("Qishui did not identify the account".to_owned()))?;
    let avatar =
        user.get("medium_avatar_url").or_else(|| user.get("avatar_url")).or_else(|| user.get("thumb_avatar_url"));
    Ok(Account { id, nickname: text(user, "nickname").unwrap_or_default(), avatar_url: image(avatar) })
}

/// `status` (`renew`): a ticket within an hour of running out is renewed, then the account is read again.
pub async fn account(client: &Client) -> Result<Option<Account>, OmniError> {
    let Some(old) = client.session().credential else { return Ok(None) };
    let mut candidate = old.clone();
    if old.expires_at < client::now_millis() + 3_600_000 {
        let refresh = client.query(
            &format!("https://open.qishui.com{PREFIX}/access/code/refresh/"),
            &[("access_code", old.access_code.clone())],
        )?;
        let (data, _) = match client.http(refresh, None, Vec::new()).await {
            Ok(answer) => answer,
            Err(OmniError::SignedOut) => return Ok(None),
            Err(err) => return Err(err),
        };
        let mut access = data.get("data").filter(|inner| inner.is_object()).cloned().unwrap_or(data);
        if access.get("open_id").is_none()
            && access.get("open_uid").is_none()
            && let Some(map) = access.as_object_mut()
        {
            map.insert("open_id".to_owned(), Value::from(old.open_id.clone()));
        }
        candidate =
            ticket(client, &access, old.guard_key.clone(), ("bd-ticket-guard-client-cert", old.guard_cert.clone()))
                .await?;
    }
    let user = match read_user(client, &candidate).await {
        Ok(user) => user,
        Err(OmniError::SignedOut) => return Ok(None),
        Err(err) => return Err(err),
    };
    if old.user_id.as_deref().is_some_and(|id| id != user.id) {
        return Ok(None);
    }
    candidate.user_id = Some(user.id.clone());
    candidate.nickname = Some(user.nickname.clone());
    candidate.avatar_url = user.avatar_url.clone();
    client.set_credential(Some(candidate));
    Ok(Some(user))
}

/// `search` for tracks: the search id kept while the cursor moves.
pub async fn search(client: &Client, query: &str, limit: u32, offset: u32) -> Result<Page<Track>, OmniError> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Page::default());
    }
    let limit = limit.clamp(1, 100) as usize;
    let search_id = client::uuid(&client.rng);
    let mut items = Vec::new();
    let mut cursor = u64::from(offset);
    while items.len() < limit {
        let body = json!({
            "q": query, "cursor": cursor.to_string(), "search_id": search_id, "from_search_id": search_id,
            "search_method": "input", "search_type": "track", "search_scene": "normal", "scene_name": "search",
            "ab_param": "", "last_viewed_media": {},
        });
        let data = client.platform("/search/", body, false).await?;
        let groups = data
            .get("result_groups")
            .and_then(Value::as_array)
            .ok_or_else(|| OmniError::Response("Qishui search groups is missing".to_owned()))?;
        let Some(group) =
            groups.iter().find(|g| g.get("id").and_then(Value::as_str) == Some("tracks")).or(groups.first())
        else {
            break;
        };
        let batch: Vec<Value> = group
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.pointer("/entity/track").cloned())
            .collect();
        let remaining = limit - items.len();
        let more = batch.len() > remaining || group.get("has_more").and_then(Value::as_bool) == Some(true);
        items.extend(batch.iter().take(remaining).filter_map(track));
        if batch.is_empty() || !more || items.len() == limit {
            break;
        }
        let next = group.get("next_cursor").and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok()));
        match next {
            Some(next) if next > cursor => cursor = next,
            _ => return Err(OmniError::Response("Search cursor did not advance".to_owned())),
        }
    }
    Ok(Page { items, total: None })
}

/// `catalog.track`: `/media/mget/` for one track.
async fn track_entity(client: &Client, id: &str) -> Result<Value, OmniError> {
    let body = json!({ "media_list": [{ "id": id, "type": "track" }], "card_context": {} });
    let data = client.platform("/media/mget/", body, false).await?;
    let list = data
        .get("media_list")
        .and_then(Value::as_array)
        .ok_or_else(|| OmniError::Response("Qishui media list is missing".to_owned()))?;
    list.iter()
        .find(|item| {
            text(item, "media_id")
                .or_else(|| {
                    item.pointer("/track_entity/base_info/id")
                        .and_then(|v| v.as_str().map(str::to_owned).or_else(|| v.as_u64().map(|n| n.to_string())))
                })
                .as_deref()
                == Some(id)
        })
        .and_then(|item| item.get("track_entity").cloned())
        .ok_or(OmniError::Unavailable)
}

/// `resolveAudio`: only plaintext resources, the full song first, then the preview clip.
pub async fn audio_source(client: &Client, id: &str) -> Result<AudioSource, OmniError> {
    let entity = track_entity(client, id).await?;
    let player = entity.get("player_info").ok_or(OmniError::Unavailable)?;
    for variant in ["full", "preview"] {
        let Some(info) = player.pointer(&format!("/{variant}/video_model_info")) else { continue };
        let mut candidates: Vec<(String, u8, f64, Option<String>)> = Vec::new();
        if let Some(model) = info.get("video_model") {
            let model = match model {
                Value::String(raw) => serde_json::from_str(raw)
                    .map_err(|_| OmniError::Response("Invalid Qishui player model".to_owned()))?,
                other => other.clone(),
            };
            for item in model.get("video_list").and_then(Value::as_array).into_iter().flatten() {
                let Some(main) = text(item, "main_url").filter(|_| !encrypted(item)) else { continue };
                let Some(url) = media_url(&main) else { continue };
                let quality = item.pointer("/video_meta/quality").and_then(Value::as_str).unwrap_or_default();
                let bitrate = item.pointer("/video_meta/bitrate").and_then(Value::as_f64).unwrap_or(0.0);
                let format = item.pointer("/video_meta/vtype").and_then(Value::as_str).map(str::to_owned);
                candidates.push((url, rank(quality), bitrate, format));
            }
        }
        if candidates.is_empty()
            && let Some(player_url) = text(info, "url_player_info").and_then(|raw| media_url(&raw))
        {
            let target = reqwest::Url::parse(&player_url).map_err(|err| OmniError::Response(err.to_string()))?;
            let (data, _) = client.http(target, None, Vec::new()).await?;
            let list = data
                .pointer("/Result/Data/PlayInfoList")
                .or_else(|| data.pointer("/data/PlayInfoList"))
                .or_else(|| data.get("PlayInfoList"));
            let list = list
                .and_then(Value::as_array)
                .ok_or_else(|| OmniError::Response("Qishui PlayInfoList is missing".to_owned()))?;
            for item in list {
                let Some(main) = text(item, "MainPlayUrl").filter(|_| !encrypted(item)) else { continue };
                let Some(url) = media_url(&main) else { continue };
                let quality = item.get("Quality").and_then(Value::as_str).unwrap_or_default();
                let format = item.get("Format").and_then(Value::as_str).map(str::to_owned);
                candidates.push((
                    url,
                    rank(quality),
                    item.get("Bitrate").and_then(Value::as_f64).unwrap_or(0.0),
                    format,
                ));
            }
        }
        if candidates.is_empty() {
            continue;
        }
        let allowed: Vec<_> = candidates.iter().filter(|c| c.1 <= WANTED_RANK).cloned().collect();
        let mut pool = if allowed.is_empty() { candidates } else { allowed };
        pool.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.total_cmp(&a.2)));
        let Some((url, _, _, format)) = pool.into_iter().next() else { continue };
        tracing::info!(preview = variant == "preview", "qishui plaintext audio issued");
        let extension = format.filter(|f| !f.is_empty()).unwrap_or_else(|| extension_of(&url));
        return Ok(AudioSource { url, extension, preview: variant == "preview" });
    }
    Err(OmniError::Unsupported("protected audio; decryption is disabled"))
}

fn rank(quality: &str) -> u8 {
    LEVELS.iter().find(|(name, _)| *name == quality).map_or(0, |(_, rank)| *rank)
}

/// `encrypted`: any sign of a protected resource.
fn encrypted(item: &Value) -> bool {
    let truthy = |value: Option<&Value>| {
        value.is_some_and(|v| {
            v.as_bool() == Some(true) || v.as_str().is_some_and(|s| !s.is_empty()) || v.as_i64().is_some_and(|n| n != 0)
        })
    };
    truthy(item.pointer("/encrypt_info/encrypt"))
        || truthy(item.pointer("/encrypt_info/spade_a"))
        || truthy(item.get("PlayAuth"))
        || truthy(item.get("Encrypt"))
}

/// `mediaUrl`: https on the car service's media hosts only.
fn media_url(raw: &str) -> Option<String> {
    const HOSTS: [&str; 18] = [
        "365yg.com",
        "douyinvod.com",
        "douyinscdn.com",
        "bytefcdn.com",
        "bytecdn.cn",
        "byteicdn.com",
        "ibytedtos.com",
        "pstatp.com",
        "snssdk.com",
        "bytedance.com",
        "bytetos.com",
        "volcvideo.com",
        "volccdn.com",
        "volces.com",
        "volcengineapi.com",
        "douyin.com",
        "qishui.com",
        "zijieapi.com",
    ];
    let url = reqwest::Url::parse(raw).ok()?;
    let host = url.host_str()?;
    let trusted = url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none_or(|port| port == 443)
        && HOSTS.iter().any(|allowed| host == *allowed || host.ends_with(&format!(".{allowed}")));
    trusted.then(|| url.into())
}

fn extension_of(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    path.rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| ext.len() <= 4)
        .unwrap_or_else(|| "m4a".to_owned())
}

/// `parseQishuiLyrics`: the track's lyric entity (KRC or LRC) with the Chinese translation.
pub async fn lyrics(client: &Client, id: &str) -> Result<Option<Lyrics>, OmniError> {
    let entity = track_entity(client, id).await?;
    let info = entity.pointer("/display_info/lyric_info").unwrap_or(&Value::Null);
    let lyric = info.get("lyric_entity").unwrap_or(&Value::Null);
    let content = text(lyric, "content").unwrap_or_default().replace('\u{0}', "");
    if content.trim().is_empty() {
        return Ok(None);
    }
    let translations = info.get("lang_translations").unwrap_or(&Value::Null);
    let translated = translations
        .get("zh-CN")
        .or_else(|| translations.get("ZH-HANS-CN"))
        .or_else(|| translations.as_object().and_then(|map| map.values().next()))
        .and_then(|t| text(t, "content"))
        .unwrap_or_default();
    let format = if lyric.get("type").and_then(Value::as_str) == Some("krc") { Format::Krc } else { Format::Lrc };
    let parsed = folia_lyrics::parse(format, &content, &translated, "");
    Ok((!parsed.lines.is_empty()).then_some(parsed))
}

/// `user_playlists`: the account's own lists and the saved ones, behind the recent-played entry the original adds.
pub async fn user_playlists(client: &Client) -> Result<Vec<Collection>, OmniError> {
    let mut all = vec![Collection {
        provider: super::ID.to_owned(),
        id: RECENT_ID.to_owned(),
        kind: CollectionKind::Playlist,
        name: "汽水最近播放".to_owned(),
        cover_url: None,
        track_count: None,
        creator: None,
        description: None,
    }];
    for (path, field) in [("/me/playlist/", "playlists"), ("/me/collection/mixed/", "mixed_collections")] {
        let mut cursor = String::new();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..MAX_PAGES {
            seen.insert(cursor.clone());
            let data = client
                .platform(path, json!({ "count": 100, "cursor": cursor, "item_types": ["playlist"] }), true)
                .await?;
            let rows = data
                .get(field)
                .or_else(|| data.get("playlist_list"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            all.extend(rows.iter().filter_map(collection));
            if data.get("has_more").and_then(Value::as_bool) != Some(true) {
                break;
            }
            let next = data
                .get("next_cursor")
                .map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()))
                .unwrap_or_default();
            if next.is_empty() || seen.contains(&next) || rows.is_empty() {
                return Err(OmniError::Response("Account playlist cursor did not advance".to_owned()));
            }
            cursor = next;
        }
    }
    let mut seen = std::collections::HashSet::new();
    Ok(all.into_iter().filter(|found| seen.insert(found.id.split('-').next().unwrap_or_default().to_owned())).collect())
}

pub async fn collection_detail(
    client: &Client,
    feed: &Feed,
    collection: &Collection,
) -> Result<CollectionDetail, OmniError> {
    let songs = match collection.kind {
        CollectionKind::Playlist if collection.id == RECENT_ID => recent_played(client).await?,
        CollectionKind::Playlist => {
            let (id, owner) = collection.id.split_once('-').unwrap_or((collection.id.as_str(), ""));
            playlist_songs(client, id, owner).await?
        }
        CollectionKind::PersonalFm => {
            let scene = collection.id.strip_prefix(SCENE_PREFIX).map(str::to_owned);
            feed.songs(client, Queue::Fm, scene, 20).await?.0
        }
        CollectionKind::Album => return Err(OmniError::Unsupported("albums")),
        CollectionKind::DailySongs => return Err(OmniError::Unsupported("daily songs")),
    };
    Ok(CollectionDetail { tracks: songs, ..Default::default() })
}

/// `playlistPage` read to its end: opaque cursors, a hundred at a time.
async fn playlist_songs(client: &Client, id: &str, owner: &str) -> Result<Vec<Track>, OmniError> {
    let mut songs = Vec::new();
    let mut cursor = "0".to_owned();
    let mut seen = std::collections::HashSet::new();
    while seen.len() < MAX_PAGES {
        seen.insert(cursor.clone());
        let body = json!({ "playlist_id": id, "owner_id": owner, "count": 100, "cursor": cursor, "type": null });
        let data = client.platform("/playlist/detail/", body, client.session().credential.is_some()).await?;
        let batch = data
            .get("media_entities")
            .and_then(Value::as_array)
            .ok_or_else(|| OmniError::Response("Qishui playlist tracks is missing".to_owned()))?;
        songs.extend(tracks(batch));
        if data.get("has_more").and_then(Value::as_bool) != Some(true) {
            break;
        }
        let next = data
            .get("next_cursor")
            .map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()))
            .unwrap_or_default();
        if batch.is_empty() || next.is_empty() || seen.contains(&next) {
            return Err(OmniError::Response("Playlist cursor did not advance".to_owned()));
        }
        cursor = next;
    }
    Ok(songs)
}

/// `recentPlayed`, every page.
async fn recent_played(client: &Client) -> Result<Vec<Track>, OmniError> {
    let mut songs = Vec::new();
    let mut cursor = String::new();
    for _ in 0..MAX_PAGES {
        let body = json!({ "count": 100, "cursor": cursor, "media_type": "track", "scene": "me_tab" });
        let data = client.platform("/me/recent-played/", body, true).await?;
        let rows = data.get("media").and_then(Value::as_array).cloned().unwrap_or_default();
        songs.extend(
            rows.iter()
                .filter_map(|row| row.get("media"))
                .filter(|media| media.get("track_entity").is_some())
                .filter_map(track),
        );
        if data.get("has_more").and_then(Value::as_bool) != Some(true) {
            break;
        }
        let next = data
            .get("next_cursor")
            .map(|v| v.as_str().map(str::to_owned).unwrap_or_else(|| v.to_string()))
            .unwrap_or_default();
        if next.is_empty() || next == cursor {
            return Err(OmniError::Response("Recent-played cursor did not advance".to_owned()));
        }
        cursor = next;
    }
    Ok(songs)
}

/// `getHomeFeed` for Qishui: personal FM from the feed, the scenes as radios; no daily songs.
pub async fn radio_feed(client: &Client, feed: &Feed, limit: u32) -> Result<RadioFeed, OmniError> {
    let fm = feed.songs(client, Queue::Fm, None, 20).await.map(|(songs, _)| songs);
    let scenes = feed_modes(client).await;
    if let (Err(err), Err(_)) = (&fm, &scenes) {
        return Err(OmniError::Response(err.to_string()));
    }
    let partial = fm.is_err() || scenes.is_err();
    let recommended = scenes.unwrap_or_default().into_iter().take(limit as usize).collect();
    Ok(RadioFeed { personal_fm: fm.unwrap_or_default(), daily_songs: Vec::new(), recommended, partial })
}

/// `modes`: the feed's scenes, each a radio played as personal FM in that scene.
async fn feed_modes(client: &Client) -> Result<Vec<Collection>, OmniError> {
    let data = client.platform("/feed/mode/", json!({}), false).await?;
    let mut seen = std::collections::HashSet::new();
    let mut scenes = Vec::new();
    for block in data.get("feed_mode_block").and_then(Value::as_array).into_iter().flatten() {
        let group = text(block, "title");
        for item in block.get("feed_mode").and_then(Value::as_array).into_iter().flatten() {
            let mode = item.pointer("/entity/feed_mode").unwrap_or(&Value::Null);
            let Some(id) = mode
                .pointer("/scene_mode/scene_mode_id")
                .and_then(|v| v.as_u64().map(|n| n.to_string()).or_else(|| v.as_str().map(str::to_owned)))
            else {
                continue;
            };
            let Some(name) = text(mode, "text") else { continue };
            if !seen.insert(id.clone()) {
                continue;
            }
            scenes.push(Collection {
                provider: super::ID.to_owned(),
                id: format!("{SCENE_PREFIX}{id}"),
                kind: CollectionKind::PersonalFm,
                name,
                cover_url: image(mode.get("url_info")),
                track_count: None,
                creator: None,
                description: group.clone(),
            });
        }
    }
    Ok(scenes)
}

/// `qishuiExplore`: one stream from the explore queue, the songs not handed out yet first.
pub async fn explore(client: &Client, feed: &Feed) -> Result<Vec<ExploreSection>, OmniError> {
    let mut fresh = Vec::new();
    let mut first = true;
    for _ in 0..3 {
        let (songs, more) = feed.songs_first(client, Queue::Explore, first, 20).await?;
        first = false;
        fresh = feed.unseen(songs);
        if !fresh.is_empty() || !more {
            break;
        }
    }
    if fresh.is_empty() {
        return Ok(Vec::new());
    }
    Ok(vec![ExploreSection {
        key: "qishui-stream".to_owned(),
        shelf: Shelf::Block("为你推荐".to_owned()),
        entries: fresh.into_iter().map(ExploreEntry::Song).collect(),
    }])
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum Queue {
    Fm,
    Explore,
}

/// `createFeed`: separate request counts per queue and scene, and the songs the explore stream already showed.
#[derive(Default)]
pub struct Feed {
    state: Mutex<FeedState>,
}

#[derive(Default)]
struct FeedState {
    counts: std::collections::HashMap<(bool, Option<String>), (u64, u64)>,
    active: std::collections::HashMap<bool, Option<String>>,
    total: u64,
    shown: std::collections::HashSet<String>,
}

impl Feed {
    pub fn clear(&self) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = FeedState::default();
    }

    async fn songs(
        &self,
        client: &Client,
        queue: Queue,
        scene: Option<String>,
        count: u32,
    ) -> Result<(Vec<Track>, bool), OmniError> {
        self.request(client, queue, scene, false, count).await
    }

    async fn songs_first(
        &self,
        client: &Client,
        queue: Queue,
        first: bool,
        count: u32,
    ) -> Result<(Vec<Track>, bool), OmniError> {
        if first {
            self.state.lock().unwrap_or_else(PoisonError::into_inner).shown.clear();
        }
        self.request(client, queue, None, first, count).await
    }

    /// `songs`: one `/feed/song-tab/` read in the queue's own count.
    async fn request(
        &self,
        client: &Client,
        queue: Queue,
        scene: Option<String>,
        first: bool,
        count: u32,
    ) -> Result<(Vec<Track>, bool), OmniError> {
        let explore = queue == Queue::Explore;
        let (state_count, selected_at, total) = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            let key = (explore, scene.clone());
            let switched = state.active.get(&explore) != Some(&scene);
            let entry = state.counts.get(&key).copied();
            let (count_so_far, selected) = match entry {
                Some(found) if !first && !switched => found,
                _ => (0, client::now_millis()),
            };
            state.total += 1;
            (count_so_far, selected, state.total)
        };
        let scene_id = scene.as_ref().and_then(|s| s.parse::<u64>().ok());
        let body = json!({
            "played_media": [], "is_first_request": state_count == 0, "with_client_cache": state_count != 0,
            "is_did_first_request": false, "count": count,
            "feed_preference": {
                "scene_mode_id": scene_id, "scene_mode_select_time": scene_id.map(|_| selected_at),
                "preference_mode": null, "total_request_count": total,
                "preference_mode_request_count": state_count + 1, "current_mode_request_count": state_count + 1,
            },
        });
        let data = client.platform("/feed/song-tab/", body, client.session().credential.is_some()).await?;
        let items = data
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| OmniError::Response("Qishui feed items are missing".to_owned()))?;
        let songs = items
            .iter()
            .filter(|item| item.get("type").and_then(Value::as_str).is_none_or(|t| t == "track"))
            .filter_map(track)
            .collect();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.active.insert(explore, scene.clone());
        state.counts.insert((explore, scene), (state_count + 1, selected_at));
        Ok((songs, data.get("has_more").and_then(Value::as_bool) == Some(true)))
    }

    fn unseen(&self, songs: Vec<Track>) -> Vec<Track> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        songs
            .into_iter()
            .filter(|song| song.online.as_ref().is_some_and(|online| state.shown.insert(online.id.clone())))
            .collect()
    }
}
