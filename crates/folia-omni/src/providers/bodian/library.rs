// Bodian's catalogue and library: search, play rights, lyrics, the QR exchange, lists, albums and the radio feed.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use folia_core::{Lyrics, Track};
use folia_lyrics::Format;
use serde_json::{Value, json};

use super::api::{Call, Client, Session};
use super::{collection, text, track, tracks};
use crate::{AudioSource, Collection, CollectionDetail, CollectionKind, LoginState, OmniError, Page, RadioFeed};

const PAGE: u32 = 100;
const MAX_PAGES: u32 = 100;
/// `BODIAN_DISCOVER_MODULE_ID`.
const DISCOVER_MODULE: u32 = 1;
const DISCOVER_PREFIX: &str = "discover-";
/// `sourceType` of saved albums in the collected list.
const ALBUM_SOURCE: u64 = 6;

fn get(params: Vec<(&str, String)>) -> Call<'_> {
    Call { params, ..Call::default() }
}

/// `search`: `search/music/list`, pages from 0.
pub async fn search(client: &Client, query: &str, limit: u32, offset: u32) -> Result<Page<Track>, OmniError> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Page::default());
    }
    let limit = limit.clamp(1, 100);
    let params = vec![
        ("keyword", query.to_owned()),
        ("correct", "1".to_owned()),
        ("pn", (offset / limit).to_string()),
        ("rn", limit.to_string()),
    ];
    let answer = client.call("/api/search/music/list", get(params)).await?;
    let data = answer.get("data").unwrap_or(&Value::Null);
    let rows = data.get("resultList").ok_or_else(|| OmniError::Response("Bodian search list is missing".to_owned()))?;
    Ok(Page { items: tracks(Some(rows)), total: data.get("total").and_then(Value::as_u64) })
}

/// `audio`: `checkRight`, then the 320k `audioUrl`; a preview (`status` 3) plays its audition clip.
pub async fn audio_source(client: &Client, id: &str) -> Result<AudioSource, OmniError> {
    let music_id: u64 = id.parse().map_err(|_| OmniError::Response(format!("not a Bodian id: {id}")))?;
    let body = json!({ "musicId": music_id, "freeSign": "" });
    let call = Call {
        params: vec![("musicId", id.to_owned()), ("freeSign", String::new())],
        body: Some(body),
        signed: true,
        ..Call::default()
    };
    let right = client.call("/api/play/music/v2/checkRight", call).await?;
    let right = right.get("data").unwrap_or(&Value::Null);
    match right.get("status").and_then(Value::as_i64) {
        Some(3) => {
            let preview = right.get("audition").unwrap_or(&Value::Null);
            let url = text(preview, &["https", "url"]).ok_or(OmniError::Unavailable)?;
            return Ok(AudioSource { url, extension: "mp3".to_owned(), preview: true });
        }
        Some(7) | None if client.session().signed_in() => return Err(OmniError::Unavailable),
        Some(7) | None => return Err(OmniError::SignedOut),
        Some(_) => {}
    }
    let device = client.session().device_id;
    let body = json!({ "devId": device, "musicId": music_id, "format": "mp3", "br": "320kmp3", "freeSign": "" });
    let call = Call {
        params: vec![
            ("devId", device),
            ("musicId", id.to_owned()),
            ("format", "mp3".to_owned()),
            ("br", "320kmp3".to_owned()),
            ("freeSign", String::new()),
        ],
        body: Some(body),
        signed: true,
        ..Call::default()
    };
    let audio = client.call("/api/play/music/v2/audioUrl", call).await?;
    let data = audio.get("data").unwrap_or(&Value::Null);
    let url = text(data, &["audioHttpsUrl", "audioUrl"])
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"));
    let url = url.ok_or_else(|| OmniError::Response("Bodian returned an invalid audio URL".to_owned()))?;
    Ok(AudioSource { url, extension: "mp3".to_owned(), preview: false })
}

/// `lyrics` with `parseBodianLyrics`: the word timing decoded and read as KRC, else the plain LRC.
pub async fn lyrics(client: &Client, id: &str) -> Result<Option<Lyrics>, OmniError> {
    let query = format!("type=lyric&req=2&lrcx=1&rid={id}&songname=&artist=&corp=kuwo&fromchannel=bodian");
    let call = Call {
        params: vec![("f", "bodian".to_owned()), ("q", STANDARD.encode(query))],
        lyric: true,
        ..Call::default()
    };
    let answer = client.call("/mobi.s", call).await?;
    let content = answer
        .pointer("/data/content")
        .and_then(Value::as_str)
        .ok_or_else(|| OmniError::Response("Bodian lyric content is missing".to_owned()))?;
    let bytes = STANDARD.decode(content).map_err(|err| OmniError::Response(format!("bodian lyric: {err}")))?;
    let content = String::from_utf8_lossy(&bytes);
    let main = strip_word_tags(&strip_kuwo_tag(&content));
    if let Some(krc) = word_timing(&content).map(|timed| awlrc_to_krc(&timed)) {
        let parsed = folia_lyrics::parse(Format::Krc, &krc, "", "");
        if !parsed.lines.is_empty() {
            return Ok(Some(parsed));
        }
    }
    let parsed = folia_lyrics::parse(Format::Lrc, &main, "", "");
    Ok((!parsed.lines.is_empty()).then_some(parsed))
}

fn strip_kuwo_tag(content: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(at) = rest.find("[kuwo:") {
        out.push_str(&rest[..at]);
        match rest[at..].find(']') {
            Some(end) => rest = &rest[at + end + 1..],
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Each `<a,b>` pair of signed integers, with where it starts and ends in `text`.
fn word_tags(text: &str) -> Vec<(usize, usize, i64, i64)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(open) = text[from..].find('<').map(|at| at + from) {
        let close = text[open..].find('>').map(|at| at + open);
        let parsed = close.and_then(|close| {
            let (a, b) = text[open + 1..close].split_once(',')?;
            let number = |s: &str| {
                (!s.is_empty() && s.trim_start_matches('-').bytes().all(|c| c.is_ascii_digit()))
                    .then(|| s.parse::<i64>().ok())
                    .flatten()
            };
            Some((close, number(a)?, number(b)?))
        });
        match parsed {
            Some((close, a, b)) => {
                found.push((open, close + 1, a, b));
                from = close + 1;
            }
            None => from = open + 1,
        }
    }
    found
}

fn strip_word_tags(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (start, end, _, _) in word_tags(text) {
        out.push_str(&text[last..start]);
        last = end;
    }
    out.push_str(&text[last..]);
    out
}

/// `decodeBodianWordTiming`: the `[kuwo:<octal>]` factors turn each obfuscated pair into `<start,duration>`.
fn word_timing(content: &str) -> Option<String> {
    let tags = word_tags(content);
    if tags.is_empty() {
        return None;
    }
    let factors = content
        .find("[kuwo:")
        .map(|at| {
            content[at + 6..].bytes().take_while(|b| (b'0'..=b'7').contains(b)).map(char::from).collect::<String>()
        })
        .filter(|octal| !octal.is_empty())
        .and_then(|octal| i64::from_str_radix(&octal, 8).ok())
        .unwrap_or(11);
    let (start_factor, duration_factor) = (factors / 10, factors % 10);
    if start_factor == 0 || duration_factor == 0 {
        return None;
    }
    let mut out = String::with_capacity(content.len());
    let mut last = 0;
    for (start, end, a, b) in tags {
        out.push_str(&content[last..start]);
        let begin = ((a + b) as f64 / (2 * start_factor) as f64).abs().trunc() as i64;
        let duration = ((a - b) as f64 / (2 * duration_factor) as f64).abs().trunc() as i64;
        out.push_str(&format!("<{begin},{duration}>"));
        last = end;
    }
    out.push_str(&content[last..]);
    Some(strip_kuwo_tag(&out))
}

/// `parseAwlrc`'s reading of `[mm:ss.xx]<offset,duration>word`, written as KRC lines `[start,duration]<offset,duration,0>`;
/// a syllable never starts before the one ahead of it ends.
fn awlrc_to_krc(text: &str) -> String {
    let mut lines = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        let Some(close) = line.strip_prefix('[').and_then(|rest| rest.find(']')) else { continue };
        let stamp = &line[1..close + 1];
        let Some(start) = awlrc_time(stamp) else { continue };
        let body = &line[close + 2..];
        let tags = word_tags(body);
        let mut cursor = 0i64;
        let mut words = String::new();
        for (index, (_, end, offset, duration)) in tags.iter().enumerate() {
            let next = tags.get(index + 1).map_or(body.len(), |tag| tag.0);
            let word = &body[*end..next];
            let offset = (*offset).max(cursor);
            let duration = (*duration).max(1);
            words.push_str(&format!("<{offset},{duration},0>{word}"));
            cursor = offset + duration;
        }
        if !words.is_empty() {
            lines.push((start, cursor, words));
        }
    }
    lines.sort_by_key(|(start, ..)| *start);
    lines.into_iter().map(|(start, length, words)| format!("[{start},{length}]{words}")).collect::<Vec<_>>().join("\n")
}

/// `parseAwlrcTimestamp`: `[h:]m:s.frac` in milliseconds.
fn awlrc_time(stamp: &str) -> Option<i64> {
    let (fields, fraction) = stamp.split_once('.')?;
    if fraction.is_empty() || fraction.len() > 3 || !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut parts: Vec<i64> = fields.split(':').map(|part| part.parse().ok()).collect::<Option<_>>()?;
    while parts.len() < 3 {
        parts.insert(0, 0);
    }
    let fraction_ms = fraction.parse::<i64>().ok()? * 10i64.pow(3 - fraction.len() as u32);
    Some(((parts[0] * 60 + parts[1]) * 60 + parts[2]) * 1000 + fraction_ms)
}

/// `login_qr_check`: status 1 waiting, 2 expired, 3 → `users/login`, where 11027 still means scanned.
pub async fn check_login(client: &Client, key: &str) -> Result<LoginState, OmniError> {
    let call = Call { params: vec![("qrCode", key.to_owned())], signed: true, anonymous: true, ..Call::default() };
    let answer = client.call("/api/ucenter/login/qrCodeStatus", call).await?;
    match answer.pointer("/data/status").and_then(|s| s.as_i64().or_else(|| s.as_str()?.parse().ok())) {
        Some(1) => return Ok(LoginState::Waiting),
        Some(2) => return Ok(LoginState::Expired),
        Some(3) => {}
        _ => return Err(OmniError::Response("Unrecognized Bodian QR status".to_owned())),
    }
    let call = Call {
        body: Some(json!({ "authType": 10, "qrCode": key })),
        post: true,
        signed: true,
        anonymous: true,
        ..Call::default()
    };
    let login = match client.call("/api/ucenter/users/login", call).await {
        Ok(answer) => answer.get("data").cloned().unwrap_or(Value::Null),
        Err(OmniError::Platform(11027)) => return Ok(LoginState::Scanned),
        Err(err) => return Err(err),
    };
    let user = login.get("userInfo").or_else(|| login.get("user")).cloned().unwrap_or(Value::Null);
    let ids: Vec<String> = [login.get("id"), login.get("uid"), user.get("id"), user.get("uid")]
        .into_iter()
        .flatten()
        .filter(|value| !value.is_null())
        .filter_map(|value| text(&json!({ "v": value }), &["v"]))
        .collect();
    let consistent = ids.first().is_some_and(|first| {
        first.bytes().all(|b| b.is_ascii_digit()) && !first.starts_with('0') && ids.iter().all(|id| id == first)
    });
    let (true, Some(uid)) = (consistent, ids.first().cloned()) else {
        return Err(OmniError::Response("Bodian returned inconsistent account identity".to_owned()));
    };
    let Some(token) = login.get("token").and_then(Value::as_str).filter(|token| !token.trim().is_empty()) else {
        return Err(OmniError::Response("Unrecognized Bodian QR status".to_owned()));
    };
    let device_id = client.session().device_id;
    let nickname = text(&user, &["nickname"]);
    let avatar_url = text(&user, &["headImg", "avatarUrl"]).map(|url| super::secure(&url));
    client.set_session(Session { device_id, uid: Some(uid), token: Some(token.to_owned()), nickname, avatar_url });
    tracing::info!("bodian sign-in confirmed");
    Ok(LoginState::Confirmed)
}

fn user_params(client: &Client) -> Result<Vec<(&'static str, String)>, OmniError> {
    let uid = client.session().uid.ok_or(OmniError::SignedOut)?;
    Ok(vec![("userId", uid.clone()), ("fromUid", uid)])
}

/// Every page of `collect/4/list` (pages from 1): the saved playlists and, under `sourceType` 6, the saved albums.
async fn collected(client: &Client) -> Result<Vec<Value>, OmniError> {
    let mut rows = Vec::new();
    for page in 1..=MAX_PAGES {
        let mut params = user_params(client)?;
        params.push(("pn", page.to_string()));
        params.push(("rn", PAGE.to_string()));
        let answer = client.call("/api/service/collect/4/list", get(params)).await?;
        let data = answer.get("data").unwrap_or(&Value::Null);
        let items = data.get("playLists").and_then(Value::as_array).cloned().unwrap_or_default();
        let count = items.len();
        rows.extend(items);
        let total = data.get("total").and_then(Value::as_u64).map(|t| t as usize);
        if count == 0 || total.map_or(count < PAGE as usize, |total| page as usize * PAGE as usize >= total) {
            break;
        }
    }
    Ok(rows)
}

/// `allPlaylists`: the liked list, the own ones (source 5), then the saved ones.
pub async fn user_playlists(client: &Client) -> Result<Vec<Collection>, OmniError> {
    let owned = client.call("/api/service/playlist/userCreate", get(user_params(client)?)).await?;
    let liked = client.call("/api/service/playlist/fond", get(user_params(client)?)).await?;
    let mut all = Vec::new();
    if let Some(found) = liked
        .get("data")
        .filter(|data| data.get("id").is_some())
        .and_then(|data| collection(data, CollectionKind::Playlist, 5))
    {
        all.push(found);
    }
    let own = owned
        .get("data")
        .map(|data| data.get("playLists").unwrap_or(data))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    all.extend(own.iter().filter_map(|row| collection(row, CollectionKind::Playlist, 5)));
    let saved = collected(client).await?;
    all.extend(
        saved
            .iter()
            .filter(|row| row.get("sourceType").and_then(Value::as_u64) != Some(ALBUM_SOURCE))
            .filter_map(|row| collection(row, CollectionKind::Playlist, 4)),
    );
    let mut seen = std::collections::HashSet::new();
    Ok(all.into_iter().filter(|found| seen.insert(found.id.clone())).collect())
}

/// `getUserAlbums`: the collected entries of `sourceType` 6.
pub async fn favorite_albums(client: &Client) -> Result<Vec<Collection>, OmniError> {
    let saved = collected(client).await?;
    Ok(saved
        .iter()
        .filter(|row| row.get("sourceType").and_then(Value::as_u64) == Some(ALBUM_SOURCE))
        .filter_map(|row| collection(row, CollectionKind::Album, 6))
        .collect())
}

pub async fn collection_detail(client: &Client, collection: &Collection) -> Result<CollectionDetail, OmniError> {
    match collection.kind {
        CollectionKind::Playlist => match collection.id.strip_prefix(DISCOVER_PREFIX) {
            Some(index) => {
                let answer =
                    client.call("/api/service/home/aiPlaylistDetail", get(vec![("index", index.to_owned())])).await?;
                Ok(CollectionDetail { tracks: tracks(answer.pointer("/data/musicList")), ..Default::default() })
            }
            None => {
                let (source, id) = collection.id.split_once('-').unwrap_or(("4", collection.id.as_str()));
                let path = format!("/api/service/playlist/{}/musicList", digits(id)?);
                let songs = song_pages(client, &path, vec![("source", source.to_owned())], 1, "list").await?;
                Ok(CollectionDetail { tracks: songs, ..Default::default() })
            }
        },
        CollectionKind::Album => {
            let id = digits(&collection.id)?;
            let songs =
                song_pages(client, &format!("/api/service/album/music/{id}"), Vec::new(), 0, "resultList").await?;
            let info = client.call(&format!("/api/service/album/{id}"), Call::default()).await.ok();
            let published = info
                .as_ref()
                .and_then(|info| info.pointer("/data/albumInfo/showtime"))
                .and_then(Value::as_str)
                .and_then(date_millis);
            Ok(CollectionDetail { tracks: songs, subscribed: None, owner: None, published })
        }
        CollectionKind::PersonalFm => Ok(CollectionDetail { tracks: personal_fm(client).await?, ..Default::default() }),
        CollectionKind::DailySongs => Err(OmniError::Unsupported("daily songs")),
    }
}

/// A song list read page by page (`pageData`), pages counted from `first`.
async fn song_pages(
    client: &Client,
    path: &str,
    extra: Vec<(&str, String)>,
    first: u32,
    field: &str,
) -> Result<Vec<Track>, OmniError> {
    let mut songs = Vec::new();
    for page in 0..MAX_PAGES {
        let mut params = extra.clone();
        params.push(("pn", (page + first).to_string()));
        params.push(("rn", PAGE.to_string()));
        let answer = client.call(path, get(params)).await?;
        let data = answer.get("data").unwrap_or(&Value::Null);
        let rows = data.get(field).and_then(Value::as_array).cloned().unwrap_or_default();
        songs.extend(rows.iter().filter_map(track));
        let total = data.get("total").and_then(Value::as_u64);
        let page_end = u64::from((page + 1) * PAGE);
        let more = !rows.is_empty() && total.map_or(rows.len() == PAGE as usize, |total| page_end < total);
        if !more {
            break;
        }
    }
    Ok(songs)
}

fn digits(id: &str) -> Result<&str, OmniError> {
    if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && !id.bytes().all(|b| b == b'0') {
        Ok(id)
    } else {
        Err(OmniError::Response("Invalid Bodian media id".to_owned()))
    }
}

/// `getHomeFeed` for Bodian: personal FM and the recommended playlists, the Discover cards first; no daily songs.
pub async fn radio_feed(client: &Client, limit: u32) -> Result<RadioFeed, OmniError> {
    let fm = personal_fm(client).await;
    let regular = client.call("/api/service/finds/playlist", Call::default()).await;
    let discover = discover_cards(client).await;
    let mut recommended = discover;
    if let Ok(answer) = &regular {
        let groups = answer.pointer("/data/lists").and_then(Value::as_array).cloned().unwrap_or_default();
        recommended.extend(
            groups
                .iter()
                .filter_map(|group| group.get("playLists").and_then(Value::as_array))
                .flatten()
                .filter_map(|row| collection(row, CollectionKind::Playlist, 4)),
        );
    }
    let mut seen = std::collections::HashSet::new();
    recommended.retain(|found| seen.insert(found.id.clone()));
    recommended.truncate(limit as usize);
    if let (Err(err), Err(_), true) = (&fm, &regular, recommended.is_empty()) {
        return Err(OmniError::Response(err.to_string()));
    }
    let partial = fm.is_err() || regular.is_err();
    Ok(RadioFeed { personal_fm: fm.unwrap_or_default(), daily_songs: Vec::new(), recommended, partial })
}

/// `getBodianDiscoverCollections`: the home module's cards, each an AI playlist named after its title.
async fn discover_cards(client: &Client) -> Vec<Collection> {
    let Ok(module) =
        client.call("/api/service/home/module", get(vec![("moduleId", DISCOVER_MODULE.to_string())])).await
    else {
        return Vec::new();
    };
    let cards = module.pointer("/data/songList").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut found = Vec::new();
    for card in cards {
        let Some(index) = card.get("id").and_then(Value::as_u64) else { continue };
        let Ok(detail) =
            client.call("/api/service/home/aiPlaylistDetail", get(vec![("index", index.to_string())])).await
        else {
            continue;
        };
        let detail = detail.get("data").cloned().unwrap_or(Value::Null);
        let songs = tracks(detail.get("musicList"));
        found.push(Collection {
            provider: super::ID.to_owned(),
            id: format!("{DISCOVER_PREFIX}{index}"),
            kind: CollectionKind::Playlist,
            name: text(&detail, &["title"])
                .or_else(|| text(&card, &["title"]))
                .unwrap_or_else(|| format!("Discover {}", index + 1)),
            cover_url: songs.first().and_then(|song| song.cover_url.clone()),
            track_count: u32::try_from(songs.len()).ok(),
            creator: None,
            description: Some(text(&detail, &["subTitle"]).unwrap_or_else(|| "「为你量身打造的专属歌单」".to_owned())),
        });
    }
    found
}

/// `personal_fm`: `music/recommendList`.
async fn personal_fm(client: &Client) -> Result<Vec<Track>, OmniError> {
    let answer = client.call("/api/service/music/recommendList", Call::default()).await?;
    Ok(tracks(answer.pointer("/data/musicList")))
}

/// `Date.parse` of `YYYY-MM-DD`.
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
    Some((era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468) * 86_400_000)
}
