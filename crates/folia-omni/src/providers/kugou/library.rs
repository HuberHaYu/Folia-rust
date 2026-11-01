// KuGou's catalogue and the signed-in account's library: search, song URLs, lists, albums and the radio cards.

use folia_core::Track;
use serde_json::{Value, json};

use super::gateway::{self, ANDROID_SALT, Body, Client, Request};
use super::{SongKey, collection, data_of, list_of, number_of, song_list_of, text_of, track, tracks};
use crate::{AudioSource, Collection, CollectionDetail, CollectionKind, OmniError, Page, RadioFeed};

/// `KUGOU_MAX_PAGE_SIZE`.
const MAX_PAGE: u32 = 100;
/// `KUGOU_LIKED_TRACK_MAX_PAGES`: a guard, not a limit of KuGou's.
const MAX_PAGES: u32 = 200;
/// `KUGOU_ANONYMOUS_SEARCH_FALLBACK_LIMIT`.
const ANONYMOUS_KEPT: usize = 100;
/// `KUGOU_RECOMMENDATION_PAGE_SIZE`.
const CARD_PAGE: u32 = 30;
/// `KUGOU_RECOMMENDATION_CARDS`: the radio's song cards, shown as playlists.
const CARDS: [(u32, &str); 6] = [
    (3006, "VIP 专属推荐"),
    (3001, "私人专属好歌"),
    (3004, "小众宝藏佳作"),
    (3014, "喜欢这首歌的 TA 也喜欢"),
    (3101, "概念 er 新推"),
    (3005, "潮流尝鲜"),
];
const CARD_PREFIX: &str = "card-";
/// `qualityValue` for `high` and its fallbacks.
const QUALITIES: [&str; 2] = ["320", "128"];

type Remembered = std::sync::Mutex<std::collections::VecDeque<String>>;

/// `searchSongs`: signed in through `search`, else anonymously; an empty signed-in first page retries anonymously.
pub async fn search(
    client: &Client,
    remembered: &Remembered,
    query: &str,
    limit: u32,
    offset: u32,
) -> Result<Page<Track>, OmniError> {
    let limit = limit.max(1);
    let page = offset / limit + 1;
    let keyword = query.trim().to_owned();
    let remembers = |keyword: &str| {
        remembered.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(|kept| kept == keyword)
    };
    if !client.session().signed_in() || (offset > 0 && remembers(&keyword)) {
        return anonymous_search(client, query, page, limit).await;
    }
    let answer = client
        .call(|| {
            Request::get("/v3/search/song")
                .router("complexsearch.kugou.com")
                .param("albumhide", 0)
                .param("iscorrection", 1)
                .param("keyword", query)
                .param("nocollect", 0)
                .param("page", page)
                .param("pagesize", limit)
                .param("platform", "AndroidFilter")
        })
        .await?;
    let rows = list_of(&answer);
    let keep = |remember: bool| {
        let mut kept = remembered.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        kept.retain(|other| *other != keyword);
        if remember {
            kept.push_back(keyword.clone());
        }
        while kept.len() > ANONYMOUS_KEPT {
            kept.pop_front();
        }
    };
    if offset == 0 && rows.is_empty() {
        tracing::warn!("kugou signed-in search returned nothing; retrying anonymously");
        keep(true);
        match anonymous_search(client, query, page, limit).await {
            Ok(found) if !found.items.is_empty() => return Ok(found),
            Ok(_) => {}
            Err(err) => tracing::warn!(error = &err as &dyn std::error::Error, "kugou anonymous retry failed"),
        }
    } else if offset == 0 {
        keep(false);
    }
    Ok(Page { items: tracks(&rows), total: total_of(&answer) })
}

/// `requestKugouAnonymousSearch`: the lite client's old search, signed without a device or a session.
async fn anonymous_search(client: &Client, query: &str, page: u32, limit: u32) -> Result<Page<Track>, OmniError> {
    let keyword =
        query.trim().split(" - ").next().map(str::trim).filter(|title| !title.is_empty()).unwrap_or(query.trim());
    let millis = gateway::now().as_millis();
    let mid = gateway::md5_hex(&millis.to_string());
    let mut params: Vec<(&str, String)> = vec![
        ("sorttype", "0".to_owned()),
        ("keyword", keyword.to_owned()),
        ("pagesize", limit.to_string()),
        ("page", page.to_string()),
        ("userid", "0".to_owned()),
        ("appid", "3116".to_owned()),
        ("token", String::new()),
        ("clienttime", (millis / 1000).to_string()),
        ("iscorrection", "1".to_owned()),
        ("uuid", "-".to_owned()),
        ("mid", mid.clone()),
        ("dfid", "-".to_owned()),
        ("clientver", "11070".to_owned()),
        ("platform", "AndroidFilter".to_owned()),
    ];
    let mut sorted: Vec<String> = params.iter().map(|(key, value)| format!("{key}={value}")).collect();
    sorted.sort_by(|a, b| a.split('=').next().cmp(&b.split('=').next()));
    params.push(("signature", gateway::md5_hex(&format!("{ANDROID_SALT}{}{ANDROID_SALT}", sorted.concat()))));
    let response = client
        .http()
        .get(gateway::with_query("http://complexsearch.kugou.com/v2/search/song", &params)?)
        .header(reqwest::header::USER_AGENT, "Android14-1070-11070-201-0-SearchSong-wifi")
        .header("KG-Rec", "1")
        .header("KG-RC", "1")
        .header("KG-CLIENTTIMEMS", millis.to_string())
        .header("mid", mid)
        .header("x-router", "complexsearch.kugou.com")
        .send()
        .await?
        .error_for_status()?;
    let answer: Value = response.json().await?;
    match answer.get("error_code").and_then(Value::as_i64) {
        Some(0 | 200) | None => {}
        Some(code) => return Err(OmniError::Platform(code)),
    }
    Ok(Page { items: tracks(&list_of(&answer)), total: total_of(&answer) })
}

fn total_of(answer: &Value) -> Option<u64> {
    let data = data_of(answer);
    ["total", "list_count", "total_count", "count"]
        .iter()
        .find_map(|key| data.get(*key).and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok())))
}

/// `getAudioSource`: each quality, by the song's album ids and then by hash alone, then the old mobile endpoint.
pub async fn audio_source(client: &Client, key: SongKey) -> Result<AudioSource, OmniError> {
    let mut first_failure = None;
    let has_ids = !key.album_id.is_empty() || !key.album_audio_id.is_empty();
    'qualities: for quality in QUALITIES {
        let variants: &[bool] = if has_ids { &[true, false] } else { &[false] };
        for &with_ids in variants {
            let (album_id, album_audio_id) =
                if with_ids { (key.album_id.as_str(), key.album_audio_id.as_str()) } else { ("", "") };
            match client.call(|| song_url(&key.hash, quality, album_id, album_audio_id)).await {
                Ok(answer) => {
                    let data = data_of(&answer);
                    let payload = data.as_array().and_then(|rows| rows.first()).unwrap_or(data);
                    let url = ["play_url", "playUrl", "url"].iter().find_map(|name| audio_url(payload.get(*name)?));
                    if let Some(url) = url {
                        tracing::info!(quality, with_ids, "kugou song url resolved");
                        let extension = extension_of(&url, payload);
                        return Ok(AudioSource { url, extension, preview: false });
                    }
                    tracing::warn!(quality, with_ids, status = ?payload.get("status"), "kugou answered no url");
                }
                Err(OmniError::SignedOut) => return Err(OmniError::SignedOut),
                Err(OmniError::Http(err)) if err.is_timeout() => {
                    first_failure.get_or_insert(OmniError::Http(err));
                    break 'qualities;
                }
                Err(err) => {
                    tracing::warn!(quality, with_ids, error = &err as &dyn std::error::Error, "kugou song url failed");
                    first_failure.get_or_insert(err);
                }
            }
        }
    }
    match legacy_play_info(client, &key.hash).await {
        Ok(Some(url)) => {
            tracing::info!("kugou song url resolved by the legacy endpoint");
            let extension = extension_of(&url, &Value::Null);
            return Ok(AudioSource { url, extension, preview: false });
        }
        Ok(None) => {}
        Err(err) => {
            tracing::warn!(error = &err as &dyn std::error::Error, "kugou legacy play info failed");
        }
    }
    Err(first_failure.filter(|err| !matches!(err, OmniError::Platform(_))).unwrap_or(OmniError::Unavailable))
}

/// `song_url`: `/v5/url` on the tracker, keyed over the hash.
fn song_url(hash: &str, quality: &str, album_id: &str, album_audio_id: &str) -> Request {
    let number = |id: &str| if id.is_empty() { "0".to_owned() } else { id.to_owned() };
    let mut request = Request::get("/v5/url")
        .router("trackercdn.kugou.com")
        .param("album_id", number(album_id))
        .param("area_code", 1)
        .param("hash", hash.to_lowercase())
        .param("ssa_flag", "is_fromtrack")
        .param("version", 11430)
        .param("page_id", 967177915)
        .param("quality", quality)
        .param("album_audio_id", number(album_audio_id))
        .param("behavior", "play")
        .param("pid", 411)
        .param("cmd", 26)
        .param("pidversion", 3001)
        .param("IsFreePart", 0)
        .param("ppage_id", "356753938,823673182,967485191")
        .param("cdnBackup", 1)
        .param("module", "")
        .param("clientver", 11430);
    request.keyed = true;
    request
}

/// `requestKugouLegacyPlayInfo`: `m.kugou.com`'s `playInfo`, for hashes `/v5/url` refuses.
async fn legacy_play_info(client: &Client, hash: &str) -> Result<Option<String>, OmniError> {
    let response = client
        .http()
        .get(gateway::with_query("https://m.kugou.com/app/i/getSongInfo.php", &[("cmd", "playInfo"), ("hash", hash)])?)
        .header(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 Chrome/124.0.0.0 Mobile Safari/537.36",
        )
        .send()
        .await?
        .error_for_status()?;
    let answer: Value = response.json().await?;
    let data = data_of(&answer);
    Ok(["url", "backup_url"]
        .iter()
        .find_map(|key| answer.get(*key).and_then(audio_url).or_else(|| data.get(*key).and_then(audio_url))))
}

/// `audioUrlOf`: the first http(s) address of a string, a comma-joined string or a list.
fn audio_url(raw: &Value) -> Option<String> {
    let values: Vec<&str> = match raw {
        Value::String(text) => vec![text.as_str()],
        Value::Array(rows) => rows.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    };
    values
        .into_iter()
        .flat_map(|value| {
            value
                .split(", http")
                .enumerate()
                .map(|(i, part)| if i == 0 { part.to_owned() } else { format!("http{part}") })
        })
        .map(|candidate| candidate.trim().to_owned())
        .find(|candidate| candidate.starts_with("https://") || candidate.starts_with("http://"))
}

fn extension_of(url: &str, payload: &Value) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let from_url =
        path.rsplit('/').next().and_then(|name| name.rsplit_once('.')).map(|(_, ext)| ext.to_ascii_lowercase());
    from_url
        .filter(|ext| matches!(ext.as_str(), "mp3" | "flac" | "m4a" | "aac" | "ogg" | "wav" | "ape"))
        .or_else(|| text_of(payload, &["extName", "ext_name"]).map(|ext| ext.to_ascii_lowercase()))
        .unwrap_or_else(|| "mp3".to_owned())
}

/// `user_playlist`, every page of the account's mixed list of playlists and saved albums.
async fn user_rows(client: &Client) -> Result<Vec<Value>, OmniError> {
    let session = client.session();
    if !session.signed_in() {
        return Err(OmniError::SignedOut);
    }
    let mut rows = Vec::new();
    for page in 1..=MAX_PAGES {
        let answer = client
            .call(|| {
                let session = client.session();
                let userid = session.userid.clone().unwrap_or_default();
                let token = session.token.clone().unwrap_or_default();
                let body = json!({
                    "userid": userid, "token": token, "total_ver": 979, "type": 2, "page": page, "pagesize": MAX_PAGE,
                });
                Request::post("/v7/get_all_list", body)
                    .router("cloudlist.service.kugou.com")
                    .param("plat", 1)
                    .param("userid", userid.parse::<u64>().unwrap_or(0))
                    .param("token", token)
            })
            .await?;
        let found = list_of(&answer);
        let count = found.len();
        rows.extend(found);
        let total = total_of(&answer).map(|total| total as usize);
        if count < MAX_PAGE as usize || total.is_some_and(|total| rows.len() >= total) {
            break;
        }
    }
    Ok(rows)
}

/// `getKugouUserCollectionType`: rows from `source` 2 or with a `musiclib_id` are saved albums.
fn is_album_row(row: &Value) -> bool {
    number_of(row, &["source"]) == Some(2.0) || row.get("musiclib_id").or_else(|| row.get("musicLibId")).is_some()
}

/// `isKugouOwnedPlaylist`.
fn is_owned(row: &Value) -> bool {
    number_of(row, &["type"]).is_none_or(|kind| kind == 0.0)
}

/// `getUserPlaylists` or `getUserAlbums`: one kind of the account's list; an own playlist without a cover borrows
/// its newest song's (`getKugouPlaylistFallbackCover`).
pub async fn user_collections(client: &Client, kind: CollectionKind) -> Result<Vec<Collection>, OmniError> {
    let rows = user_rows(client).await?;
    let mut all = Vec::new();
    for row in rows.iter().filter(|row| is_album_row(row) == (kind == CollectionKind::Album)) {
        let Some(mut found) = collection(row, kind) else { continue };
        if kind == CollectionKind::Playlist && found.cover_url.is_none() && is_owned(row) {
            found.cover_url = playlist_page(client, &found.id, 1, 1)
                .await
                .ok()
                .and_then(|answer| tracks(&list_of(&answer)).into_iter().find_map(|song| song.cover_url));
        }
        all.push(found);
    }
    Ok(all)
}

/// `playlist_track_all`: one page of a playlist by its global collection id.
async fn playlist_page(client: &Client, id: &str, page: u32, size: u32) -> Result<Value, OmniError> {
    client
        .call(|| {
            Request::get("/pubsongs/v2/get_other_list_file_nofilt")
                .param("area_code", 1)
                .param("begin_idx", (page - 1) * size)
                .param("plat", 1)
                .param("type", 1)
                .param("mode", 1)
                .param("personal_switch", 1)
                .param("extend_fields", "abtags,hot_cmt,popularization")
                .param("pagesize", size)
                .param("global_collection_id", id)
        })
        .await
}

pub async fn collection_detail(client: &Client, collection: &Collection) -> Result<CollectionDetail, OmniError> {
    match collection.kind {
        CollectionKind::Playlist => match collection.id.strip_prefix(CARD_PREFIX).and_then(|id| id.parse().ok()) {
            Some(card) => {
                Ok(CollectionDetail { tracks: card_songs(client, card, CARD_PAGE).await?, ..Default::default() })
            }
            None => playlist_detail(client, &collection.id).await,
        },
        CollectionKind::Album => album_detail(client, &collection.id).await,
        CollectionKind::DailySongs => Ok(CollectionDetail { tracks: daily_songs(client).await?, ..Default::default() }),
        CollectionKind::PersonalFm => Ok(CollectionDetail { tracks: personal_fm(client).await?, ..Default::default() }),
    }
}

/// Every page of a playlist (`getKugouPlaylistTrackSongs`), with whether the account saved it and who made it.
async fn playlist_detail(client: &Client, id: &str) -> Result<CollectionDetail, OmniError> {
    let mut songs = Vec::new();
    for page in 1..=MAX_PAGES {
        let answer = playlist_page(client, id, page, MAX_PAGE).await?;
        let rows = list_of(&answer);
        songs.extend(rows.iter().filter_map(track));
        let total = number_of(data_of(&answer), &["total", "total_count", "count"]).filter(|t| *t > MAX_PAGE as f64);
        if rows.len() < MAX_PAGE as usize || total.is_some_and(|total| songs.len() as f64 >= total) {
            break;
        }
    }
    let rows = if client.session().signed_in() { user_rows(client).await.unwrap_or_default() } else { Vec::new() };
    let row = rows.iter().find(|row| {
        !is_album_row(row) && text_of(row, &["global_collection_id", "globalCollectionId"]).as_deref() == Some(id)
    });
    let owner = match row {
        Some(row) => text_of(row, &["list_create_userid", "create_userid", "userid"]),
        None => playlist_info(client, id)
            .await
            .ok()
            .and_then(|info| text_of(&info, &["list_create_userid", "create_userid"])),
    };
    Ok(CollectionDetail { tracks: songs, subscribed: Some(row.is_some()), owner, published: None })
}

/// `playlist_detail`: the list's own record.
async fn playlist_info(client: &Client, id: &str) -> Result<Value, OmniError> {
    let answer = client
        .call(|| {
            let session = client.session();
            let body = json!({
                "data": [{ "global_collection_id": id }],
                "userid": session.userid.clone().unwrap_or_else(|| "0".to_owned()),
                "token": session.token.clone().unwrap_or_default(),
            });
            Request::post("/v3/get_list_info", body).router("pubsongs.kugou.com")
        })
        .await?;
    list_of(&answer).into_iter().next().ok_or_else(|| OmniError::Response("kugou playlist has no record".to_owned()))
}

/// `album_detail`: the album's record on the open API.
async fn album_info(client: &Client, id: &str) -> Result<Value, OmniError> {
    let answer = client
        .call(|| {
            let body = json!({
                "data": [{ "album_id": id }],
                "is_buy": 0,
                "fields": "album_id,album_name,publish_date,sizable_cover,intro,language,is_publish,heat,type,quality,authors,exclusive,author_name,trans_param",
            });
            let mut request = Request::post("/kmr/v2/albums", body).router("openapi.kugou.com");
            request.headers.push(("kg-tid", "255"));
            request
        })
        .await?;
    list_of(&answer).into_iter().next().ok_or_else(|| OmniError::Response("kugou album has no record".to_owned()))
}

/// `album_songs` page after page, with the release date and whether the account saved the album.
async fn album_detail(client: &Client, id: &str) -> Result<CollectionDetail, OmniError> {
    const PAGE: u32 = 30;
    let mut songs = Vec::new();
    for page in 1..=MAX_PAGES {
        let answer = client
            .call(|| {
                let body = json!({ "album_id": id, "is_buy": "", "page": page, "pagesize": PAGE });
                let mut request = Request::post("/v1/album_audio/lite", body).router("openapi.kugou.com");
                request.headers.push(("kg-tid", "255"));
                request
            })
            .await?;
        let rows = list_of(&answer);
        songs.extend(rows.iter().filter_map(track));
        let total = number_of(data_of(&answer), &["total", "total_count", "count"]);
        if rows.len() < PAGE as usize || total.is_some_and(|total| songs.len() as f64 >= total) {
            break;
        }
    }
    let info = album_info(client, id).await.ok();
    let published = info
        .as_ref()
        .and_then(|info| text_of(info, &["publish_date", "publish_time", "release_date"]))
        .and_then(|date| date_millis(&date));
    let rows = if client.session().signed_in() { user_rows(client).await.unwrap_or_default() } else { Vec::new() };
    let subscribed = rows.iter().any(|row| is_album_row(row) && album_row_id(row).as_deref() == Some(id));
    Ok(CollectionDetail { tracks: songs, subscribed: Some(subscribed), owner: None, published })
}

fn album_row_id(row: &Value) -> Option<String> {
    text_of(row, &["musiclib_id", "musicLibId", "album_id", "albumId", "list_create_listid"])
}

/// `normalizeTimestamp` for a `YYYY-MM-DD` date, a year, or seconds or milliseconds.
fn date_millis(text: &str) -> Option<i64> {
    let text = text.trim();
    if let Ok(number) = text.parse::<i64>() {
        return match number {
            1..=9999 => days_from_civil(number, 1, 1).map(|days| days * 86_400_000),
            n if n < 1_000_000_000_000 => Some(n * 1000),
            n => Some(n),
        };
    }
    let mut parts = text.get(..10)?.split('-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
    days_from_civil(year, month, day).map(|days| days * 86_400_000)
}

/// Days from 1970-01-01 to a proleptic Gregorian date.
fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// `subscribePlaylist` and `subscribeAlbum`: `playlist_add` copies the list into the account, `playlist_del` drops it.
pub async fn set_subscribed(client: &Client, collection: &Collection, subscribed: bool) -> Result<(), OmniError> {
    let album = match collection.kind {
        CollectionKind::Album => true,
        CollectionKind::Playlist if !collection.id.starts_with(CARD_PREFIX) => false,
        _ => return Err(OmniError::Response("this collection cannot be saved".to_owned())),
    };
    if !subscribed {
        let rows = user_rows(client).await?;
        let row = rows.iter().find(|row| {
            is_album_row(row) == album
                && if album {
                    album_row_id(row).as_deref() == Some(collection.id.as_str())
                } else {
                    text_of(row, &["global_collection_id", "globalCollectionId", "listid"]).as_deref()
                        == Some(collection.id.as_str())
                }
        });
        let Some(listid) = row.and_then(|row| text_of(row, &["listid", "list_id"])) else { return Ok(()) };
        return delete_list(client, &listid).await;
    }
    let (name, creator, listid, gid, source) = if album {
        let info = album_info(client, &collection.id).await?;
        let author = info
            .get("authors")
            .and_then(Value::as_array)
            .and_then(|list| list.first())
            .map(|a| a.get("base").unwrap_or(a));
        let creator = text_of(&info, &["list_create_userid", "create_userid", "author_id", "authorId", "userid"])
            .or_else(|| author.and_then(|author| text_of(author, &["author_id", "authorId", "id"])))
            .ok_or_else(|| OmniError::Response("kugou album has no author".to_owned()))?;
        let name = text_of(&info, &["name", "album_name", "AlbumName"]).unwrap_or_else(|| collection.name.clone());
        let id = text_of(&info, &["album_id", "AlbumID", "albumId"]).unwrap_or_else(|| collection.id.clone());
        (name, creator, id, String::new(), 2)
    } else {
        let info = playlist_info(client, &collection.id).await?;
        let creator = text_of(&info, &["create_userid", "list_create_userid", "userid"]).unwrap_or_default();
        let listid =
            text_of(&info, &["create_listid", "list_create_listid", "listid"]).unwrap_or_else(|| collection.id.clone());
        let gid = text_of(&info, &["gid", "list_create_gid", "global_collection_id"]).unwrap_or_default();
        (collection.name.clone(), creator, listid, gid, 1)
    };
    client
        .call(|| {
            let session = client.session();
            let body = json!({
                "userid": session.userid.clone().unwrap_or_default(),
                "token": session.token.clone().unwrap_or_default(),
                "total_ver": 0, "name": name, "type": 1, "source": source, "is_pri": 0,
                "list_create_userid": creator, "list_create_listid": listid, "list_create_gid": gid,
                "from_shupinmv": 0,
            });
            Request::post("/cloudlist.service/v5/add_list", body)
        })
        .await
        .map(|_| ())
}

/// `playlist_del`: the list id AES-wrapped, its key RSA-sealed in `p`, signed by `signParamsKey`.
async fn delete_list(client: &Client, listid: &str) -> Result<(), OmniError> {
    let session = client.session();
    let id: u64 = listid.parse().map_err(|_| OmniError::Response(format!("not a KuGou list id: {listid}")))?;
    let (key, sealed) = gateway::playlist_aes_encrypt(&json!({ "listid": id, "total_ver": 0, "type": 1 }).to_string());
    let p = gateway::rsa_pkcs1(&gateway::key_envelope(&key, &session))?.to_uppercase();
    let clienttime = gateway::now().as_secs().to_string();
    let request = Request {
        post: true,
        body: Body::Text(sealed),
        ..Request::get("/v2/delete_list")
            .router("cloudlist.service.kugou.com")
            .param("clienttime", &clienttime)
            .param("key", gateway::sign_params_key(&clienttime))
            .param("last_area", "gztx")
            .param("clientver", gateway::CLIENTVER)
            .param("appid", gateway::APPID)
            .param("last_time", &clienttime)
            .param("p", p)
    };
    let raw = client.send_raw(request).await?;
    use base64::Engine;
    let answer = gateway::playlist_aes_decrypt(&key, &base64::engine::general_purpose::STANDARD.encode(raw))?;
    gateway::refusal(&answer).map_or(Ok(()), Err)
}

/// `getHomeFeed`: personal FM, the daily songs and the song cards; each may fail alone.
pub async fn radio_feed(client: &Client, limit: u32) -> Result<RadioFeed, OmniError> {
    let fm = personal_fm(client).await;
    let daily = daily_songs(client).await;
    let size = limit.clamp(1, CARD_PAGE);
    let mut recommended = Vec::new();
    let mut cards_failed = 0;
    for (card, name) in CARDS {
        match card_songs(client, card, size).await {
            Ok(songs) if !songs.is_empty() => recommended.push(Collection {
                provider: super::ID.to_owned(),
                id: format!("{CARD_PREFIX}{card}"),
                kind: CollectionKind::Playlist,
                name: name.to_owned(),
                cover_url: songs.iter().find_map(|song| song.cover_url.clone()),
                track_count: u32::try_from(songs.len()).ok(),
                creator: None,
                description: None,
            }),
            Ok(_) => {}
            Err(err) => {
                cards_failed += 1;
                tracing::warn!(card, error = &err as &dyn std::error::Error, "kugou recommendation card failed");
            }
        }
    }
    if let (Err(err), Err(_), true) = (&fm, &daily, cards_failed == CARDS.len()) {
        return Err(OmniError::Response(err.to_string()));
    }
    let partial = fm.is_err() || daily.is_err() || cards_failed > 0;
    Ok(RadioFeed { personal_fm: fm.unwrap_or_default(), daily_songs: daily.unwrap_or_default(), recommended, partial })
}

/// `top_card_youth`: one song card.
async fn card_songs(client: &Client, card: u32, size: u32) -> Result<Vec<Track>, OmniError> {
    let answer = client
        .call(|| {
            Request::post(
                "youth/v1/song/single_card_recommend",
                json!({ "tagid": "", "u_info": "", "source_mixsong": "" }),
            )
            .param("card_id", card)
            .param("area_code", 1)
            .param("platform", "ios")
            .param("module_id", 1)
            .param("ver", "v2")
            .param("pagesize", size)
            .param("clientver", 11490)
        })
        .await?;
    Ok(tracks(&song_list_of(&answer)))
}

/// `everyday_recommend`.
async fn daily_songs(client: &Client) -> Result<Vec<Track>, OmniError> {
    let answer = client
        .call(|| Request {
            post: true,
            ..Request::get("/everyday_song_recommend").router("everydayrec.service.kugou.com").param("platform", "ios")
        })
        .await?;
    Ok(tracks(&song_list_of(&answer)))
}

/// `personal_fm`: KuGou's personal radio, signed by `signParamsKey` over the time in milliseconds.
async fn personal_fm(client: &Client) -> Result<Vec<Track>, OmniError> {
    let answer = client
        .call(|| {
            let session = client.session();
            let millis = gateway::now().as_millis();
            let mut body = json!({
                "appid": gateway::APPID.parse::<u64>().unwrap_or(0), "clienttime": millis as u64, "mid": session.mid,
                "action": "play", "recommend_source_locked": 0, "song_pool_id": 0, "callerid": 0, "m_type": 1,
                "platform": "ios", "area_code": 1, "remain_songcnt": 0,
                "clientver": gateway::CLIENTVER.parse::<u64>().unwrap_or(0), "is_overplay": 0, "mode": "normal",
                "fakem": "ca981cfc583a4c37f28d2d49000013c16a0a", "key": gateway::sign_params_key(&millis.to_string()),
            });
            if let (Some(map), Some(userid)) = (body.as_object_mut(), session.userid.clone()) {
                map.insert("userid".to_owned(), Value::from(userid.clone()));
                map.insert("kguid".to_owned(), Value::from(userid));
            }
            if let (Some(map), Some(token)) = (body.as_object_mut(), session.token.clone()) {
                map.insert("token".to_owned(), Value::from(token));
            }
            Request::post("/v2/personal_recommend", body).router("persnfm.service.kugou.com")
        })
        .await?;
    Ok(tracks(&song_list_of(&answer)))
}
