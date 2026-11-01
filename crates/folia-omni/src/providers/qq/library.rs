// QQ Music's catalogue and the signed-in library: search, play links, QRC lyrics, lists, albums, radio, explore.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use folia_core::{Lyrics, Track};
use folia_lyrics::Format;
use serde_json::{Value, json};

use super::musicu::{self, Client};
use super::{SongKey, album, pick, playlist, qrc, text, track, tracks};
use crate::{
    Account, AudioSource, Collection, CollectionDetail, CollectionKind, ExploreEntry, ExploreSection, OmniError, Page,
    RadioFeed, Shelf,
};

/// `QQ_SEARCH_MAX_KEYWORD_LENGTH` and `QQ_SEARCH_MAX_PAGE_SIZE`.
const MAX_KEYWORD: usize = 60;
const MAX_PAGE: u32 = 50;
const ANONYMOUS_KEPT: usize = 100;
/// `QQ_QUALITY_FALLBACKS.high`: the file prefix and extension of each tier.
const TIERS: [(&str, &str); 2] = [("M800", "mp3"), ("M500", "mp3")];
const FAVORITE_LIMIT: u64 = 1000;
const FAVORITE_PAGE: u64 = 100;
const SONG_LIST_PAGE: u64 = 1000;
const TOPLIST_PREFIX: &str = "toplist-";
const DIR_PREFIX: &str = "dir-";

type Remembered = std::sync::Mutex<std::collections::VecDeque<String>>;

/// `searchQqSongs`: with a session through the Android face, else (or when that page is empty) anonymously.
pub async fn search(
    client: &Client,
    remembered: &Remembered,
    query: &str,
    limit: u32,
    offset: u32,
) -> Result<Page<Track>, OmniError> {
    let keyword: String = query.trim().chars().take(MAX_KEYWORD).collect();
    if keyword.is_empty() {
        return Ok(Page::default());
    }
    let size = limit.clamp(1, MAX_PAGE);
    let page = offset / size + 1;
    let remembers =
        || remembered.lock().unwrap_or_else(std::sync::PoisonError::into_inner).iter().any(|k| *k == keyword);
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
    if client.credential().is_some() && !(offset > 0 && remembers()) {
        let param = search_param(&keyword, page, size, 0);
        match client.android("music.search.SearchCgiService", "DoSearchForQQMusicLite", param).await {
            Ok(data) => {
                let found = search_page(&data);
                if !found.items.is_empty() || offset > 0 {
                    if offset == 0 {
                        keep(false);
                    }
                    return Ok(found);
                }
                tracing::warn!("qq session search returned nothing; retrying anonymously");
                keep(true);
                return Ok(anonymous_search(client, &keyword, page, size).await.unwrap_or(found));
            }
            Err(err) => {
                tracing::warn!(
                    error = &err as &dyn std::error::Error,
                    "qq session search unavailable; anonymous instead"
                );
                keep(true);
            }
        }
    }
    anonymous_search(client, &keyword, page, size).await
}

/// `searchQQSongs` of `qqLyricProvider`: the lite app's search, type 7.
async fn anonymous_search(client: &Client, keyword: &str, page: u32, size: u32) -> Result<Page<Track>, OmniError> {
    let data = client
        .light("music.search.SearchCgiService", "DoSearchForQQMusicLite", search_param(keyword, page, size, 7))
        .await?;
    Ok(search_page(&data))
}

fn search_param(keyword: &str, page: u32, size: u32, kind: u32) -> Value {
    let search_id = (musicu::now_millis() % 86_400_000) as u64 + u64::from_be_bytes(random8()) % 100_000_000_000_000;
    json!({
        "search_id": search_id.to_string(), "remoteplace": "search.android.keyboard", "query": keyword,
        "search_type": kind, "num_per_page": size, "page_num": page, "highlight": 0, "nqc_flag": 0, "page_id": 1, "grp": 1,
    })
}

fn random8() -> [u8; 8] {
    let mut word = [0u8; 8];
    word.copy_from_slice(&musicu::random_bytes(8));
    word
}

fn search_page(data: &Value) -> Page<Track> {
    let total = data.pointer("/meta/sum").or_else(|| data.pointer("/meta/estimate_sum")).and_then(Value::as_u64);
    Page { items: tracks(data.pointer("/body/item_song")), total }
}

/// `getAudioSource`: 320 then 128; signed in through `UrlGetVkey`, as a guest through the web's `CgiGetVkey`.
pub async fn audio_source(client: &Client, key: SongKey) -> Result<AudioSource, OmniError> {
    if key.mid.is_empty() {
        return Err(OmniError::Response("QQ song has no songmid".to_owned()));
    }
    let media = if key.media_mid.is_empty() { key.mid.clone() } else { key.media_mid.clone() };
    let signed_in = client.credential().is_some();
    let mut failure = None;
    for (prefix, extension) in TIERS {
        let filename = format!("{prefix}{}{media}.{extension}", key.mid);
        let answer = if signed_in {
            signed_vkey(client, &filename, &key.mid).await
        } else {
            client.guest_vkey(&filename, &key.mid).await
        };
        match answer {
            Ok(data) => {
                let purl = data
                    .get("midurlinfo")
                    .and_then(Value::as_array)
                    .and_then(|rows| {
                        rows.iter()
                            .find(|row| text(row, &["songmid"]).as_deref() == Some(key.mid.as_str()))
                            .or(rows.first())
                    })
                    .and_then(|row| text(row, &["purl"]));
                if let Some(purl) = purl {
                    let url = format!("{}{purl}", stream_domain(&data));
                    tracing::info!(prefix, signed_in, "qq play link resolved");
                    return Ok(AudioSource { url, extension: extension.to_owned(), preview: false });
                }
                tracing::warn!(prefix, "qq gave an empty play link");
            }
            Err(OmniError::SignedOut) => return Err(OmniError::SignedOut),
            Err(err) => {
                tracing::warn!(prefix, error = &err as &dyn std::error::Error, "qq play link failed");
                failure.get_or_insert(err);
            }
        }
    }
    Err(failure.unwrap_or(OmniError::Unavailable))
}

/// `getAuthenticatedPlayUrls` for one songmid at one quality.
async fn signed_vkey(client: &Client, filename: &str, songmid: &str) -> Result<Value, OmniError> {
    let credential = client.credential().ok_or(OmniError::SignedOut)?;
    let guid = musicu::random_hex(32);
    let param = json!({
        "filename": [filename], "guid": guid, "songmid": [songmid], "songtype": [0], "uin": credential.musicid, "ctx": 0,
    });
    client.android("music.vkey.GetVkey", "UrlGetVkey", param).await
}

/// `streamDomainOf`: the first `sip` not on `ws` hosts, else the default stream host.
fn stream_domain(data: &Value) -> String {
    let sip: Vec<&str> = data
        .get("sip")
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let domain = sip
        .iter()
        .find(|value| !value.starts_with("http://ws"))
        .or(sip.first())
        .copied()
        .unwrap_or("http://dl.stream.qqmusic.qq.com/");
    domain.strip_prefix("http://").map(|rest| format!("https://{rest}")).unwrap_or_else(|| domain.to_owned())
}

/// `fetchQQLyrics`: `GetPlayLyricInfo` with the song's names, QRC when it is word-timed, translation and romaji.
pub async fn lyrics(client: &Client, key: SongKey) -> Result<Option<Lyrics>, OmniError> {
    if key.id.is_empty() || key.mid.is_empty() {
        tracing::warn!("qq lyrics need both the song id and mid");
        return Ok(None);
    }
    // The original passes the song it holds; here its names are read back by the songmid.
    let detail = client
        .anonymous("music.pf_song_detail_svr", "get_song_detail_yqq", json!({ "song_type": 0, "song_mid": key.mid }))
        .await
        .ok()
        .and_then(|data| data.get("track_info").cloned())
        .unwrap_or(Value::Null);
    let song = track(&detail);
    let b64 = |value: &str| STANDARD.encode(value.as_bytes());
    let artists = song.as_ref().and_then(|song| song.artist.clone()).unwrap_or_default().replace(" / ", ", ");
    let param = json!({
        "albumName": b64(song.as_ref().and_then(|song| song.album.as_deref()).unwrap_or_default()),
        "crypt": 1, "ct": 19, "cv": 2111,
        "interval": song.as_ref().map_or(0, |song| song.duration as u64),
        "lrc_t": 0, "qrc": 1, "qrc_t": 0, "roma": 1, "roma_t": 0,
        "singerName": b64(&artists),
        "songID": key.id.parse::<u64>().unwrap_or(0),
        "songName": b64(song.as_ref().map_or("", |song| song.title.as_str())),
        "trans": 1, "trans_t": 0, "type": 0,
    });
    let data = client.light("music.musichallSong.PlayLyricInfo", "GetPlayLyricInfo", param).await?;
    let hex = |name: &str| data.get(name).and_then(Value::as_str).filter(|value| !value.is_empty());
    let Some(main) = hex("lyric") else { return Ok(None) };
    let lyric = qrc::decrypt(main)?;
    let translation = hex("trans").and_then(|value| qrc::decrypt(value).ok()).unwrap_or_default();
    let romanization = hex("roma").and_then(|value| qrc::decrypt(value).ok()).unwrap_or_default();
    let is_qrc = lyric.contains('(') && lyric.contains(')') && has_qrc_line(&lyric);
    let format = if is_qrc { Format::Qrc } else { Format::detect(&lyric) };
    let parsed = folia_lyrics::parse(format, &lyric, &translation, &romanization);
    Ok((!parsed.lines.is_empty()).then_some(parsed))
}

/// `/\[\d+,\d+\]/`.
fn has_qrc_line(text: &str) -> bool {
    text.match_indices('[').any(|(at, _)| {
        let rest = &text[at + 1..];
        let first = rest.bytes().take_while(u8::is_ascii_digit).count();
        let after = &rest[first..];
        first > 0 && after.starts_with(',') && {
            let second = after[1..].bytes().take_while(u8::is_ascii_digit).count();
            second > 0 && after[1 + second..].starts_with(']')
        }
    })
}

/// `getLoginStatus`: `GetLoginUserInfo`, its account fields under `info`.
pub async fn account(client: &Client) -> Result<Option<Account>, OmniError> {
    let Some(credential) = client.credential() else { return Ok(None) };
    let data = match client.android("music.UserInfo.userInfoServer", "GetLoginUserInfo", json!({})).await {
        Ok(data) => data,
        Err(OmniError::SignedOut) => return Ok(None),
        Err(err) => return Err(err),
    };
    let info = data.get("info").unwrap_or(&Value::Null);
    let nickname = text(&data, &["nickname", "nick", "name", "userName"])
        .or_else(|| text(info, &["nickname", "nick", "name", "userName"]))
        .unwrap_or_default();
    let avatar_url = text(&data, &["avatarUrl"]).or_else(|| text(info, &["logo", "avatarUrl"]));
    Ok(Some(Account { id: credential.musicid, nickname, avatar_url }))
}

/// `getPlaylists`: the account's own playlists, then the saved ones page by page.
pub async fn user_playlists(client: &Client) -> Result<Vec<Collection>, OmniError> {
    let credential = client.credential().ok_or(OmniError::SignedOut)?;
    let created = client
        .android("music.musicasset.PlaylistBaseRead", "GetPlaylistByUin", json!({ "uin": credential.musicid }))
        .await?;
    let mut rows: Vec<Value> = created.get("v_playlist").and_then(Value::as_array).cloned().unwrap_or_default();
    if let Some(encrypted) = credential.encrypt_uin.clone() {
        let mut offset = 0;
        while offset < FAVORITE_LIMIT {
            let param = json!({ "uin": encrypted, "offset": offset, "size": FAVORITE_PAGE });
            let page = client.android("music.musicasset.PlaylistFavRead", "CgiGetPlaylistFavInfo", param).await?;
            let items = page.get("v_list").and_then(Value::as_array).cloned().unwrap_or_default();
            let count = items.len() as u64;
            rows.extend(items);
            let more = page.get("hasmore").is_some_and(|more| more.as_bool() == Some(true) || more.as_i64() == Some(1));
            if !more || count == 0 {
                break;
            }
            offset += count;
        }
    }
    let mut seen = std::collections::HashSet::new();
    Ok(rows.iter().filter_map(playlist).filter(|found| seen.insert(found.id.clone())).collect())
}

pub async fn collection_detail(client: &Client, collection: &Collection) -> Result<CollectionDetail, OmniError> {
    match collection.kind {
        CollectionKind::Playlist => match collection.id.strip_prefix(TOPLIST_PREFIX) {
            Some(id) => toplist_detail(client, id).await,
            None => playlist_detail(client, &collection.id).await,
        },
        CollectionKind::Album => album_detail(client, &collection.id).await,
        CollectionKind::DailySongs => Ok(CollectionDetail { tracks: daily_songs(client).await?, ..Default::default() }),
        CollectionKind::PersonalFm => {
            Ok(CollectionDetail { tracks: guess_you_like(client).await?, ..Default::default() })
        }
    }
}

/// `loadRawPlaylistTracks`: an owned directory through `CgiGetDiss`, a list through `uniform_get_Dissinfo` when signed
/// in, else the anonymous `fcg_ucc_getcdinfo_byids_cp`.
async fn playlist_detail(client: &Client, id: &str) -> Result<CollectionDetail, OmniError> {
    let credential = client.credential();
    if let (Some(dir), Some(credential)) = (id.strip_prefix(DIR_PREFIX), credential.as_ref()) {
        let encrypted = credential
            .encrypt_uin
            .clone()
            .ok_or_else(|| OmniError::Response("Login credential is missing encryptUin".to_owned()))?;
        let mut songs = Vec::new();
        let mut begin = 0u64;
        loop {
            let param = json!({
                "disstid": 0, "dirid": dir.parse::<u64>().unwrap_or(0), "tag": true, "song_begin": begin, "song_num": 100,
                "userinfo": true, "orderlist": true, "enc_host_uin": encrypted,
            });
            let data = client.android("music.srfDissInfo.DissInfo", "CgiGetDiss", param).await?;
            let page = tracks(data.get("songlist"));
            let count = page.len() as u64;
            songs.extend(page);
            let total = data.get("total_song_num").and_then(Value::as_u64).unwrap_or(0);
            if count == 0 || begin + count >= total {
                break;
            }
            begin += count;
        }
        return Ok(CollectionDetail {
            tracks: songs,
            subscribed: None,
            owner: Some(credential.musicid.clone()),
            published: None,
        });
    }
    let disstid: u64 = id.parse().map_err(|_| OmniError::Response(format!("not a QQ playlist id: {id}")))?;
    if credential.is_some() {
        let mut songs = Vec::new();
        let mut first = Value::Null;
        for page in 0..10 {
            let param = json!({
                "disstid": disstid, "userinfo": 1, "tag": 1, "orderlist": 1, "song_begin": songs.len(),
                "song_num": SONG_LIST_PAGE, "onlysonglist": 0, "enc_host_uin": "",
            });
            let data = client.android("music.srfDissInfo.aiDissInfo", "uniform_get_Dissinfo", param).await?;
            let page_songs = tracks(data.get("songlist"));
            let more = data.get("hasmore").is_some_and(|more| more.as_i64() == Some(1) || more.as_bool() == Some(true));
            let empty = page_songs.is_empty();
            songs.extend(page_songs);
            if page == 0 {
                first = data;
            }
            let total = first.get("total_song_num").and_then(Value::as_u64);
            if !more || empty || total.is_some_and(|total| songs.len() as u64 >= total) {
                break;
            }
        }
        let owner = first
            .pointer("/dirinfo/creator/musicid")
            .and_then(musicu::identifier)
            .or_else(|| first.pointer("/dirinfo/host_uin").and_then(musicu::identifier));
        let subscribed = playlist_fan(client, disstid).await.ok();
        return Ok(CollectionDetail { tracks: songs, subscribed, owner, published: None });
    }
    let params = [
        ("disstid", disstid.to_string()),
        ("format", "json".to_owned()),
        ("outCharset", "utf-8".to_owned()),
        ("type", "1".to_owned()),
        ("json", "1".to_owned()),
        ("utf8", "1".to_owned()),
        ("onlysong", "0".to_owned()),
        ("new_format", "1".to_owned()),
    ];
    let answer = client.legacy("/qzone/fcg-bin/fcg_ucc_getcdinfo_byids_cp.fcg", &params).await?;
    let list = answer.pointer("/cdlist/0").unwrap_or(&Value::Null);
    Ok(CollectionDetail {
        tracks: tracks(list.get("songlist")),
        subscribed: None,
        owner: text(list, &["uin"]),
        published: None,
    })
}

/// `IsPlaylistFan`.
async fn playlist_fan(client: &Client, tid: u64) -> Result<bool, OmniError> {
    let data = client.android("music.musicasset.PlaylistFavRead", "IsPlaylistFan", json!({ "v_tid": [tid] })).await?;
    data.pointer(&format!("/m_fan/{tid}"))
        .and_then(Value::as_bool)
        .ok_or_else(|| OmniError::Response("no fan state".to_owned()))
}

/// `IsAlbumFan`.
async fn album_fan(client: &Client, mid: &str) -> Result<bool, OmniError> {
    let credential = client.credential().ok_or(OmniError::SignedOut)?;
    let uin =
        credential.musicid.parse::<u64>().map(Value::from).unwrap_or_else(|_| Value::from(credential.musicid.clone()));
    let data = client
        .android("music.musicasset.AlbumFavRead", "IsAlbumFan", json!({ "uin": uin, "v_albumMid": [mid] }))
        .await?;
    data.pointer(&format!("/m_fan/{mid}"))
        .and_then(Value::as_bool)
        .ok_or_else(|| OmniError::Response("no fan state".to_owned()))
}

/// `getAlbumInfo` (`fcg_v8_album_info_cp`).
async fn album_detail(client: &Client, mid: &str) -> Result<CollectionDetail, OmniError> {
    let params = [("albummid", mid.to_owned()), ("format", "json".to_owned()), ("outCharset", "utf-8".to_owned())];
    let answer = client.legacy("/v8/fcg-bin/fcg_v8_album_info_cp.fcg", &params).await?;
    if let Some(code) = answer.get("code").and_then(Value::as_i64).filter(|code| *code != 0) {
        return Err(OmniError::Platform(code));
    }
    let data = answer.get("data").unwrap_or(&Value::Null);
    let published = text(data, &["aDate", "publishDate", "publictime"]).and_then(|date| date_millis(&date));
    let subscribed = if client.credential().is_some() { album_fan(client, mid).await.ok() } else { None };
    Ok(CollectionDetail { tracks: tracks(data.get("list")), subscribed, owner: None, published })
}

/// `GetDetail` of a chart, its first hundred songs.
async fn toplist_detail(client: &Client, id: &str) -> Result<CollectionDetail, OmniError> {
    let topid: u64 = id.parse().map_err(|_| OmniError::Response(format!("not a QQ chart id: {id}")))?;
    let param = json!({ "topid": topid, "offset": 0, "num": 100, "period": "" });
    let data = client.anonymous("musicToplist.ToplistInfoServer", "GetDetail", param).await?;
    Ok(CollectionDetail { tracks: tracks(data.get("songInfoList")), ..Default::default() })
}

/// `user_playlist_subscribe` (`FavPlaylist`/`CancelFavPlaylist`) and `user_album_subscribe` (`FavAlbum`/`CancelFavAlbum`).
pub async fn set_subscribed(client: &Client, collection: &Collection, subscribed: bool) -> Result<(), OmniError> {
    let credential = client.credential().ok_or(OmniError::SignedOut)?;
    match collection.kind {
        CollectionKind::Playlist
            if !collection.id.starts_with(TOPLIST_PREFIX) && !collection.id.starts_with(DIR_PREFIX) =>
        {
            let tid: u64 = collection.id.parse().map_err(|_| OmniError::Response("not a QQ playlist id".to_owned()))?;
            let method = if subscribed { "FavPlaylist" } else { "CancelFavPlaylist" };
            let param = json!({ "uin": credential.musicid, "v_playlistId": [tid] });
            client.android("music.musicasset.PlaylistFavWrite", method, param).await.map(|_| ())
        }
        CollectionKind::Album => {
            if album_fan(client, &collection.id).await.ok() == Some(subscribed) {
                return Ok(());
            }
            let method = if subscribed { "FavAlbum" } else { "CancelFavAlbum" };
            let param = json!({ "uin": credential.musicid, "v_albumMid": [collection.id] });
            let data = client.android("music.musicasset.AlbumFavWrite", method, param).await?;
            match data.get("result").and_then(Value::as_i64) {
                Some(0) => Ok(()),
                other => Err(OmniError::Platform(other.unwrap_or(-1))),
            }
        }
        _ => Err(OmniError::Response("this collection cannot be saved".to_owned())),
    }
}

/// `getFavoriteAlbums` (`fcg_get_profile_order_asset`, request type 2), every page.
pub async fn favorite_albums(client: &Client) -> Result<Vec<Collection>, OmniError> {
    let credential = client.credential().ok_or(OmniError::SignedOut)?;
    let mut all = Vec::new();
    let mut offset = 0u64;
    loop {
        let params = [
            ("ct", "20".to_owned()),
            ("cid", "205360956".to_owned()),
            ("userid", credential.musicid.clone()),
            ("reqtype", "2".to_owned()),
            ("sin", offset.to_string()),
            ("ein", (offset + FAVORITE_PAGE - 1).to_string()),
            ("format", "json".to_owned()),
        ];
        let answer = client.legacy("/fav/fcgi-bin/fcg_get_profile_order_asset.fcg", &params).await?;
        match answer.get("code").and_then(Value::as_i64).unwrap_or(0) {
            0 => {}
            1000 | 104401 | 104400 => return Err(OmniError::SignedOut),
            code => return Err(OmniError::Platform(code)),
        }
        let data = answer.get("data").unwrap_or(&Value::Null);
        let rows = data.get("albumlist").and_then(Value::as_array).cloned().unwrap_or_default();
        let count = rows.len() as u64;
        all.extend(rows.iter().filter_map(album));
        let total = data.get("totalalbum").and_then(Value::as_u64).unwrap_or(0);
        if count == 0 || offset + count >= total || offset + count >= FAVORITE_LIMIT {
            return Ok(all);
        }
        offset += count;
    }
}

/// `getHomeFeed`: guess-you-like as personal FM, the daily songs and the recommended playlists.
pub async fn radio_feed(client: &Client, limit: u32) -> Result<RadioFeed, OmniError> {
    let fm = guess_you_like(client).await;
    let daily = daily_songs(client).await;
    let recommended = recommended_playlists(client, limit).await;
    if let (Err(err), Err(_), Err(_)) = (&fm, &daily, &recommended) {
        return Err(OmniError::Response(err.to_string()));
    }
    for err in [fm.as_ref().err(), daily.as_ref().err(), recommended.as_ref().err()].into_iter().flatten() {
        tracing::warn!(error = err as &dyn std::error::Error, "qq home feed part failed");
    }
    let partial = fm.is_err() || daily.is_err() || recommended.is_err();
    Ok(RadioFeed {
        personal_fm: fm.unwrap_or_default(),
        daily_songs: daily.unwrap_or_default(),
        recommended: recommended.unwrap_or_default(),
        partial,
    })
}

/// `GUESS_YOU_LIKE`: radio 99.
async fn guess_you_like(client: &Client) -> Result<Vec<Track>, OmniError> {
    let data = client
        .android("music.radioProxy.MbTrackRadioSvr", "get_radio_track", json!({ "id": 99, "firstplay": 1, "num": 15 }))
        .await?;
    Ok(tracks(data.get("tracks")))
}

/// `DAILY_SONGS`: directory 202.
async fn daily_songs(client: &Client) -> Result<Vec<Track>, OmniError> {
    let param = json!({ "dirid": 202, "optype": 3, "tag": 1, "userinfo": 0, "pic_dpi": 300 });
    let data = client.android("music.srfDissInfo.aiDissInfo", "uniform_get_Dissinfo", param).await?;
    Ok(tracks(data.get("songlist")))
}

/// `recommendPlaylistsResult`: `GetRecommendFeed`, at most 60.
async fn recommended_playlists(client: &Client, limit: u32) -> Result<Vec<Collection>, OmniError> {
    let param = json!({ "From": 0, "Size": limit.clamp(1, 60) });
    let module = ("music.playlist.PlaylistSquare", "GetRecommendFeed");
    let data = if client.credential().is_some() {
        client.android(module.0, module.1, param).await?
    } else {
        client.anonymous(module.0, module.1, param).await?
    };
    let rows = data.get("List").and_then(Value::as_array).cloned().unwrap_or_default();
    Ok(rows.iter().filter_map(|row| row.pointer("/Playlist/basic")).filter_map(basic_playlist).collect())
}

/// The `basic` node of the playlist square and the categories.
fn basic_playlist(basic: &Value) -> Option<Collection> {
    let cover = basic.get("cover").unwrap_or(&Value::Null);
    let picurl = ["default_url", "big_url", "medium_url", "small_url"].iter().find_map(|key| text(cover, &[key]));
    let mut row = json!({ "tid": basic.get("tid"), "title": basic.get("title"), "desc": basic.get("desc"), "picurl": picurl, "songnum": basic.get("song_cnt") });
    if let (Some(map), Some(creator)) = (row.as_object_mut(), basic.get("creator")) {
        map.insert("creator".to_owned(), creator.clone());
    }
    playlist(&row)
}

/// The explore page from what the original's QQ feed reads first: the daily songs, new songs, new albums and charts.
pub async fn explore(client: &Client) -> Result<Vec<ExploreSection>, OmniError> {
    let mut sections = Vec::new();
    let mut failures = 0;
    if client.credential().is_some() {
        match daily_songs(client).await {
            Ok(songs) if !songs.is_empty() => sections.push(section(
                "qq-daily-songs",
                Shelf::DailySongs,
                songs.into_iter().map(ExploreEntry::Song).collect(),
            )),
            Ok(_) => {}
            Err(err) => {
                failures += 1;
                tracing::warn!(error = &err as &dyn std::error::Error, "qq daily songs unavailable");
            }
        }
    }
    match client.anonymous("newsong.NewSongServer", "get_new_song_info", json!({ "type": 5 })).await {
        Ok(data) => {
            let songs: Vec<ExploreEntry> =
                tracks(data.get("songlist")).into_iter().take(24).map(ExploreEntry::Song).collect();
            if !songs.is_empty() {
                sections.push(section("qq-new-songs", Shelf::Block("新歌速递".to_owned()), songs));
            }
        }
        Err(err) => {
            failures += 1;
            tracing::warn!(error = &err as &dyn std::error::Error, "qq new songs unavailable");
        }
    }
    match client
        .anonymous(
            "newalbum.NewAlbumServer",
            "get_new_album_info",
            json!({ "area": 1, "sin": 0, "start": 0, "num": 20 }),
        )
        .await
    {
        Ok(data) => {
            let albums: Vec<ExploreEntry> = data
                .get("albums")
                .and_then(Value::as_array)
                .map(|rows| rows.iter().filter_map(new_album).map(ExploreEntry::Collection).collect())
                .unwrap_or_default();
            if !albums.is_empty() {
                sections.push(section("qq-new-albums", Shelf::Block("新碟上架".to_owned()), albums));
            }
        }
        Err(err) => {
            failures += 1;
            tracing::warn!(error = &err as &dyn std::error::Error, "qq new albums unavailable");
        }
    }
    match client.anonymous("musicToplist.ToplistInfoServer", "GetAll", json!({})).await {
        Ok(data) => {
            let charts: Vec<ExploreEntry> = data
                .get("group")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|group| group.get("toplist").and_then(Value::as_array))
                .flatten()
                .filter_map(chart)
                .map(ExploreEntry::Collection)
                .collect();
            if !charts.is_empty() {
                sections.push(section("qq-charts", Shelf::Charts, charts));
            }
        }
        Err(err) => {
            failures += 1;
            tracing::warn!(error = &err as &dyn std::error::Error, "qq charts unavailable");
        }
    }
    if sections.is_empty() && failures > 0 {
        return Err(OmniError::Response("qq explore page".to_owned()));
    }
    Ok(sections)
}

fn section(key: &str, shelf: Shelf, entries: Vec<ExploreEntry>) -> ExploreSection {
    ExploreSection { key: key.to_owned(), shelf, entries }
}

/// One `get_new_album_info` album: `mid`, `name`, `singers`.
fn new_album(raw: &Value) -> Option<Collection> {
    let mid = text(raw, &["mid", "albumMid"])?;
    let mut row = json!({ "albummid": mid, "name": raw.get("name"), "singers": raw.get("singers") });
    if let (Some(map), Some(photo)) = (row.as_object_mut(), pick(raw, &["photo"]).and_then(|p| text(p, &["pic_mid"]))) {
        map.insert("picUrl".to_owned(), Value::from(format!("https://y.gtimg.cn/music/photo_new/T002M000{photo}.jpg")));
    }
    album(&row)
}

/// `toChartInfo`: a chart as a playlist whose songs `GetDetail` reads.
fn chart(raw: &Value) -> Option<Collection> {
    let id = raw.get("topId").and_then(Value::as_u64).filter(|id| *id > 0)?;
    Some(Collection {
        provider: super::ID.to_owned(),
        id: format!("{TOPLIST_PREFIX}{id}"),
        kind: CollectionKind::Playlist,
        name: text(raw, &["title"])?,
        cover_url: text(raw, &["headPicUrl", "frontPicUrl", "musichallPicUrl"]),
        track_count: raw.get("totalNum").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()),
        creator: None,
        description: text(raw, &["updateTime", "intro"]),
    })
}

/// A `YYYY-MM-DD` date in milliseconds since the epoch.
fn date_millis(text: &str) -> Option<i64> {
    let mut parts = text.get(..10)?.split('-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * 86_400_000)
}
