// Spotify's catalogue and the account's library through pathfinder and spclient, as the original's adapter reads them.

use folia_core::{Lyrics, Track};
use folia_lyrics::Format;
use serde_json::{Value, json};

use super::api::{Client, SPCLIENT_URL, User};
use super::{album_of, playlist_of, song_of, spotify_id};
use crate::{
    Account, Collection, CollectionDetail, CollectionKind, ExploreEntry, ExploreSection, OmniError, Page, RadioFeed,
    Shelf,
};

pub const LIKED_ID: &str = "liked";
/// `MAX_ROWS` of a playlist or album page.
const MAX_ROWS: u64 = 100;
const MAX_PAGES: u64 = 100;
const SEARCH_ROWS: u32 = 50;
/// `MADE_FOR_ACCOUNT`: the browse page of the lists made for the account.
const MADE_FOR_ACCOUNT: &str = "spotify:page:0JQ5DAt0tbjZptfcdMSKl3";
/// `DAILY_MIX_ID`.
const DAILY_MIX_PREFIX: &str = "37i9dQZF1E3";
const HOME_ROWS: u64 = 10;
/// Home sections that are the account's own listening record (`LISTENING_RECORD_SECTIONS`).
const LISTENING_RECORD: [&str; 2] = ["HomeRecentlyPlayedSectionData", "HomeShortsSectionData"];

/// `searchTracks`.
pub async fn search(client: &Client, query: &str, limit: u32, offset: u32) -> Result<Page<Track>, OmniError> {
    let text = query.trim();
    if text.is_empty() {
        return Ok(Page::default());
    }
    let variables = json!({
        "searchTerm": text, "offset": offset, "limit": limit.clamp(1, SEARCH_ROWS), "includePreReleases": false,
        "numberOfTopResults": 20, "includeAudiobooks": true, "includeAuthors": false,
    });
    let data = client.query("searchTracks", variables, None).await?;
    let tracks = data.pointer("/searchV2/tracksV2").unwrap_or(&Value::Null);
    let items = tracks
        .get("items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| row.pointer("/item/data"))
        .filter_map(song_of)
        .collect();
    Ok(Page { items, total: tracks.get("totalCount").and_then(Value::as_u64) })
}

/// `readAccount`: `profileAttributes` for the name and picture, `product_state` for the plan.
pub async fn read_account(client: &Client, token: Option<&str>) -> Result<User, OmniError> {
    let profile = client.query("profileAttributes", json!({}), token).await?;
    let state = client
        .call(&format!("{SPCLIENT_URL}/melody/v1/product_state?market=from_token"), None, token, &[])
        .await
        .ok()
        .flatten();
    let me = profile
        .pointer("/me/profile")
        .ok_or_else(|| OmniError::Response("Spotify did not name the account".to_owned()))?;
    let username = me
        .get("username")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| OmniError::Response("Spotify did not name the account".to_owned()))?;
    let avatar = me
        .pointer("/avatar/sources")
        .and_then(Value::as_array)
        .and_then(|sources| {
            sources.iter().max_by_key(|source| source.get("width").and_then(Value::as_u64).unwrap_or(0))
        })
        .and_then(|source| source.get("url").and_then(Value::as_str))
        .map(str::to_owned);
    let premium = state.map(|state| {
        let field = |key: &str| state.get(key).and_then(Value::as_str).map(str::to_owned);
        field("catalogue").as_deref() == Some("premium") || field("product").as_deref() == Some("premium")
    });
    Ok(User {
        id: public_account_id(username),
        nickname: me
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.is_empty())
            .unwrap_or("Spotify")
            .to_owned(),
        avatar_url: avatar,
        premium,
    })
}

/// `publicAccountId`: the user name never leaves the provider.
fn public_account_id(username: &str) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, format!("folia-spotify:{username}").as_bytes());
    digest.as_ref().iter().take(12).map(|b| format!("{b:02x}")).collect()
}

/// `login_status`: the kept account, read again from Spotify.
pub async fn account(client: &Client) -> Result<Option<Account>, OmniError> {
    let Some(session) = client.session() else { return Ok(None) };
    let user = match read_account(client, None).await {
        Ok(user) => user,
        Err(OmniError::SignedOut) => return Ok(None),
        Err(err) => {
            tracing::warn!(error = &err as &dyn std::error::Error, "spotify account could not be read again");
            session.user
        }
    };
    if let Some(mut kept) = client.session() {
        kept.user = user.clone();
        client.set_session(Some(kept));
    }
    Ok(Some(Account { id: user.id, nickname: user.nickname, avatar_url: user.avatar_url }))
}

/// `color-lyrics`: line-synced lyrics as LRC; Spotify's unsynced lyrics have no times to show them by.
pub async fn lyrics(client: &Client, id: &str) -> Result<Option<Lyrics>, OmniError> {
    let url = format!("{SPCLIENT_URL}/color-lyrics/v2/track/{id}?format=json&vocalRemoval=false&market=from_token");
    let Some(answer) = client.call(&url, None, None, &[("app-platform", "WebPlayer")]).await? else { return Ok(None) };
    let lyrics = answer.get("lyrics").unwrap_or(&Value::Null);
    if lyrics.get("syncType").and_then(Value::as_str) != Some("LINE_SYNCED") {
        return Ok(None);
    }
    let lrc: String = lyrics
        .get("lines")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|line| {
            let ms = line.get("startTimeMs").and_then(|v| v.as_u64().or_else(|| v.as_str()?.parse().ok())).unwrap_or(0);
            let words = line.get("words").and_then(Value::as_str).unwrap_or_default();
            format!("[{:02}:{:02}.{:02}]{words}\n", ms / 60_000, ms / 1000 % 60, ms % 1000 / 10)
        })
        .collect();
    let parsed = folia_lyrics::parse(Format::Lrc, &lrc, "", "");
    Ok((!parsed.lines.is_empty()).then_some(parsed))
}

/// `getUserPlaylists`: the liked songs first, then `libraryV3`'s playlists page by page.
pub async fn user_playlists(client: &Client) -> Result<Vec<Collection>, OmniError> {
    let liked = client.query("fetchLibraryTracks", json!({ "offset": 0, "limit": 1 }), None).await?;
    let tracks = liked.pointer("/me/library/tracks").unwrap_or(&Value::Null);
    let latest = tracks.pointer("/items/0/track/data").and_then(song_of);
    let mut all = vec![Collection {
        provider: super::ID.to_owned(),
        id: LIKED_ID.to_owned(),
        kind: CollectionKind::Playlist,
        name: "已点赞的歌曲".to_owned(),
        cover_url: latest.and_then(|song| song.cover_url),
        track_count: tracks.get("totalCount").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()),
        creator: None,
        description: None,
    }];
    all.extend(library_items(client, "Playlists").await?.iter().filter_map(playlist_of));
    Ok(all)
}

/// `libraryV3` of one kind, every page.
async fn library_items(client: &Client, kind: &str) -> Result<Vec<Value>, OmniError> {
    let mut rows = Vec::new();
    for page in 0..MAX_PAGES {
        let variables = json!({
            "filters": [kind], "order": null, "textFilter": "", "features": [], "limit": 50, "offset": page * 50,
            "flatten": true, "expandedFolders": [], "folderUri": null, "includeFoldersWhenFlattening": true,
        });
        let data = client.query("libraryV3", variables, None).await?;
        let library = data.pointer("/me/libraryV3").unwrap_or(&Value::Null);
        let items: Vec<Value> = library.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
        let count = items.len() as u64;
        rows.extend(items.iter().filter_map(|row| row.pointer("/item/data").cloned()));
        let total = library.get("totalCount").and_then(Value::as_u64).unwrap_or(0);
        if count == 0 || (page + 1) * 50 >= total {
            break;
        }
    }
    Ok(rows)
}

/// `getUserAlbums`.
pub async fn favorite_albums(client: &Client) -> Result<Vec<Collection>, OmniError> {
    Ok(library_items(client, "Albums").await?.iter().filter_map(album_of).collect())
}

pub async fn collection_detail(client: &Client, collection: &Collection) -> Result<CollectionDetail, OmniError> {
    match collection.kind {
        CollectionKind::Playlist if collection.id == LIKED_ID => {
            Ok(CollectionDetail { tracks: liked_songs(client).await?, ..Default::default() })
        }
        CollectionKind::Playlist => playlist_detail(client, &collection.id).await,
        CollectionKind::Album => album_detail(client, &collection.id).await,
        CollectionKind::DailySongs => Ok(CollectionDetail { tracks: daily_songs(client).await?, ..Default::default() }),
        CollectionKind::PersonalFm => Ok(CollectionDetail { tracks: personal_fm(client).await?, ..Default::default() }),
    }
}

/// `fetchLibraryTracks`, every page.
async fn liked_songs(client: &Client) -> Result<Vec<Track>, OmniError> {
    let mut songs = Vec::new();
    for page in 0..MAX_PAGES {
        let data =
            client.query("fetchLibraryTracks", json!({ "offset": page * MAX_ROWS, "limit": MAX_ROWS }), None).await?;
        let tracks = data.pointer("/me/library/tracks").unwrap_or(&Value::Null);
        let items = tracks
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| OmniError::Response("Spotify liked songs are missing".to_owned()))?;
        songs.extend(items.iter().filter_map(|row| row.pointer("/track/data")).filter_map(song_of));
        let total = tracks.get("totalCount").and_then(Value::as_u64).unwrap_or(0);
        if items.is_empty() || (page + 1) * MAX_ROWS >= total {
            break;
        }
    }
    Ok(songs)
}

/// `fetchPlaylist`, page after page; whether the account follows it, and whether it is the account's own.
async fn playlist_detail(client: &Client, id: &str) -> Result<CollectionDetail, OmniError> {
    let uri = format!("spotify:playlist:{id}");
    let mut songs = Vec::new();
    let mut head = Value::Null;
    for page in 0..MAX_PAGES {
        let variables =
            json!({ "uri": uri, "offset": page * MAX_ROWS, "limit": MAX_ROWS, "enableWatchFeedEntrypoint": false });
        let data = client.query("fetchPlaylist", variables, None).await?;
        let list = data.get("playlistV2").cloned().unwrap_or(Value::Null);
        let content =
            list.get("content").ok_or_else(|| OmniError::Response(format!("Spotify has no playlist {id}")))?;
        let items = content.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
        songs.extend(items.iter().filter_map(|row| row.pointer("/itemV2/data")).filter_map(song_of));
        let total = content.get("totalCount").and_then(Value::as_u64).unwrap_or(0);
        if page == 0 {
            head = list;
        }
        if items.is_empty() || (page + 1) * MAX_ROWS >= total {
            break;
        }
    }
    let owned = head.pointer("/currentUserCapabilities/canEditItems").and_then(Value::as_bool) == Some(true);
    let subscribed = if owned { None } else { head.get("following").and_then(Value::as_bool) };
    let owner = client.session().filter(|_| owned).map(|session| session.user.id);
    Ok(CollectionDetail { tracks: songs, subscribed, owner, published: None })
}

/// `getAlbum`: its rows name no album, so they get this one's; `areEntitiesInLibrary` for whether it is kept.
async fn album_detail(client: &Client, id: &str) -> Result<CollectionDetail, OmniError> {
    let uri = format!("spotify:album:{id}");
    let mut songs = Vec::new();
    let mut album = Value::Null;
    for page in 0..MAX_PAGES {
        let data = client
            .query("getAlbum", json!({ "uri": uri, "locale": "", "offset": page * MAX_ROWS, "limit": MAX_ROWS }), None)
            .await?;
        let union = data.get("albumUnion").cloned().unwrap_or(Value::Null);
        let rows = union.pointer("/tracksV2/items").and_then(Value::as_array).cloned().unwrap_or_default();
        if page == 0 {
            album = union.clone();
        }
        let card = album_of(&album);
        songs.extend(rows.iter().filter_map(|row| row.get("track")).filter_map(song_of).map(|mut song| {
            if let Some(card) = &card {
                song.album = Some(card.name.clone());
                song.cover_url = card.cover_url.clone();
            }
            song
        }));
        let total = union.pointer("/tracksV2/totalCount").and_then(Value::as_u64).unwrap_or(0);
        if rows.is_empty() || (page + 1) * MAX_ROWS >= total {
            break;
        }
    }
    let saved = client.query("areEntitiesInLibrary", json!({ "uris": [uri] }), None).await.ok();
    let subscribed = saved.and_then(|answer| answer.pointer("/lookup/0/data/saved").and_then(Value::as_bool));
    let published = album.pointer("/date/isoString").and_then(Value::as_str).and_then(iso_millis).or_else(|| {
        album.pointer("/date/year").and_then(Value::as_i64).map(|year| days_from_civil(year, 1, 1) * 86_400_000)
    });
    Ok(CollectionDetail { tracks: songs, subscribed, owner: None, published })
}

/// `library`: an album with `addToLibrary`, someone else's playlist with `addItemsToRootlist` (or their removals).
pub async fn set_subscribed(client: &Client, collection: &Collection, subscribed: bool) -> Result<(), OmniError> {
    match collection.kind {
        CollectionKind::Album => {
            let name = if subscribed { "addToLibrary" } else { "removeFromLibrary" };
            client.change(name, json!({ "libraryItemUris": [format!("spotify:album:{}", collection.id)] })).await
        }
        CollectionKind::Playlist if collection.id != LIKED_ID => {
            let uri = format!("spotify:playlist:{}", collection.id);
            let head = client
                .query(
                    "fetchPlaylist",
                    json!({ "uri": uri, "offset": 0, "limit": 1, "enableWatchFeedEntrypoint": false }),
                    None,
                )
                .await?;
            let list = head.get("playlistV2").unwrap_or(&Value::Null);
            if list.pointer("/currentUserCapabilities/canAdministratePermissions").and_then(Value::as_bool)
                == Some(true)
            {
                return Err(OmniError::Response("The playlist is the account's own".to_owned()));
            }
            let name = if subscribed { "addItemsToRootlist" } else { "removeItemsFromRootlist" };
            client.change(name, json!({ "uris": [uri] })).await
        }
        _ => Err(OmniError::Response("this collection cannot be saved".to_owned())),
    }
}

/// `spotifyMadeForAccount`: the playlists of the made-for-you browse page, each once.
async fn made_for_account(client: &Client) -> Result<Vec<Collection>, OmniError> {
    let variables = json!({
        "uri": MADE_FOR_ACCOUNT, "pagePagination": { "offset": 0, "limit": 20 }, "sectionPagination": { "offset": 0, "limit": 20 },
        "browseEndUserIntegration": "INTEGRATION_WEB_PLAYER", "includeEpisodeContentRatingsV2": true,
    });
    let data = client.query("browsePage", variables, None).await?;
    let mut seen = std::collections::HashSet::new();
    Ok(sections_of(data.pointer("/browse/sections/items"))
        .into_iter()
        .flat_map(|section| cards_of(&section))
        .filter_map(|card| match card {
            ExploreEntry::Collection(found) if found.kind == CollectionKind::Playlist => Some(found),
            _ => None,
        })
        .filter(|found| seen.insert(found.id.clone()))
        .collect())
}

/// `getDailySongs`: the first daily mix among the lists made for the account.
async fn daily_songs(client: &Client) -> Result<Vec<Track>, OmniError> {
    let lists = made_for_account(client).await?;
    let Some(daily) = lists.iter().find(|list| list.id.starts_with(DAILY_MIX_PREFIX)) else { return Ok(Vec::new()) };
    Ok(playlist_detail(client, &daily.id).await?.tracks)
}

/// `getPersonalFm`: the radio of a liked song (`inspiredby-mix`).
async fn personal_fm(client: &Client) -> Result<Vec<Track>, OmniError> {
    let liked = client.query("fetchLibraryTracks", json!({ "offset": 0, "limit": 50 }), None).await?;
    let seeds: Vec<String> = liked
        .pointer("/me/library/tracks/items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| row.pointer("/track/_uri").and_then(Value::as_str))
        .filter_map(|uri| spotify_id(uri, "track"))
        .collect();
    let pick = super::api::now_millis() as usize % seeds.len().max(1);
    for seed in seeds.iter().cycle().skip(pick).take(seeds.len().min(3)) {
        let url =
            format!("{SPCLIENT_URL}/inspiredby-mix/v2/seed_to_playlist/spotify:track:{seed}?response-format=json");
        let Some(answer) = client.call(&url, None, None, &[]).await? else { continue };
        let Some(list) =
            answer.pointer("/mediaItems/0/uri").and_then(Value::as_str).and_then(|uri| spotify_id(uri, "playlist"))
        else {
            continue;
        };
        let songs: Vec<Track> = playlist_detail(client, &list)
            .await?
            .tracks
            .into_iter()
            .filter(|song| song.online.as_ref().is_some_and(|o| o.id != *seed))
            .collect();
        if !songs.is_empty() {
            return Ok(songs);
        }
    }
    Ok(Vec::new())
}

/// `getHomeFeed`: personal FM, the daily mix and the other lists made for the account.
pub async fn radio_feed(client: &Client, limit: u32) -> Result<RadioFeed, OmniError> {
    let fm = personal_fm(client).await;
    let made = made_for_account(client).await;
    let daily = match &made {
        Ok(lists) => match lists.iter().find(|list| list.id.starts_with(DAILY_MIX_PREFIX)) {
            Some(mix) => playlist_detail(client, &mix.id).await.map(|detail| detail.tracks),
            None => Ok(Vec::new()),
        },
        Err(err) => Err(OmniError::Response(err.to_string())),
    };
    if let (Err(err), Err(_)) = (&fm, &made) {
        return Err(OmniError::Response(err.to_string()));
    }
    let partial = fm.is_err() || made.is_err() || daily.is_err();
    let recommended = made
        .unwrap_or_default()
        .into_iter()
        .filter(|list| !list.id.starts_with(DAILY_MIX_PREFIX))
        .take(limit as usize)
        .collect();
    Ok(RadioFeed { personal_fm: fm.unwrap_or_default(), daily_songs: daily.unwrap_or_default(), recommended, partial })
}

/// The explore page's first read: Spotify's home sections (without the listening record), then the new releases
/// of followed artists and the made-for-you sections the home page did not show.
pub async fn explore(client: &Client) -> Result<Vec<ExploreSection>, OmniError> {
    let home = client
        .query(
            "home",
            json!({ "timeZone": "Asia/Shanghai", "sp_t": "", "facet": "", "sectionItemsLimit": HOME_ROWS }),
            None,
        )
        .await?;
    let mut sections: Vec<ExploreSection> = sections_of(home.pointer("/home/sectionContainer/sections/items"))
        .into_iter()
        .filter(|row| {
            !LISTENING_RECORD.contains(&row.pointer("/data/__typename").and_then(Value::as_str).unwrap_or_default())
        })
        .filter_map(|row| section_of(&row))
        .collect();
    let releases = client
        .query(
            "queryWhatsNewFeed",
            json!({ "offset": 0, "limit": 50, "onlyUnPlayedItems": false, "includedContentTypes": ["ALBUM"] }),
            None,
        )
        .await;
    match releases {
        Ok(data) => {
            let albums: Vec<ExploreEntry> = data
                .pointer("/whatsNewFeedItems/items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(card_of)
                .collect();
            if !albums.is_empty() {
                sections.push(ExploreSection {
                    key: "spotify-new-releases".to_owned(),
                    shelf: Shelf::Block("关注歌手的新发行".to_owned()),
                    entries: albums,
                });
            }
        }
        Err(err) => tracing::warn!(error = &err as &dyn std::error::Error, "spotify new releases unavailable"),
    }
    let variables = json!({
        "uri": MADE_FOR_ACCOUNT, "pagePagination": { "offset": 0, "limit": 20 }, "sectionPagination": { "offset": 0, "limit": 20 },
        "browseEndUserIntegration": "INTEGRATION_WEB_PLAYER", "includeEpisodeContentRatingsV2": true,
    });
    match client.query("browsePage", variables, None).await {
        Ok(data) => {
            for section in sections_of(data.pointer("/browse/sections/items")).iter().filter_map(section_of) {
                if !sections.iter().any(|shown| shown.key == section.key) {
                    sections.push(section);
                }
            }
        }
        Err(err) => tracing::warn!(error = &err as &dyn std::error::Error, "spotify made-for-you page unavailable"),
    }
    Ok(sections)
}

fn sections_of(list: Option<&Value>) -> Vec<Value> {
    list.and_then(Value::as_array).cloned().unwrap_or_default()
}

/// `sectionOf`: a section with the playlists, albums and artists the app shows; none without any.
fn section_of(row: &Value) -> Option<ExploreSection> {
    let key = row.get("uri").and_then(Value::as_str)?;
    let entries = cards_of(row);
    if entries.is_empty() {
        return None;
    }
    let title = row.pointer("/data/title/transformedLabel").and_then(Value::as_str).unwrap_or_default();
    Some(ExploreSection { key: key.to_owned(), shelf: Shelf::Block(title.to_owned()), entries })
}

fn cards_of(row: &Value) -> Vec<ExploreEntry> {
    row.pointer("/sectionItems/items").and_then(Value::as_array).into_iter().flatten().filter_map(card_of).collect()
}

/// `itemOf`: a playlist or an album card (artists have no page here).
fn card_of(card: &Value) -> Option<ExploreEntry> {
    let data = card.pointer("/content/data")?;
    match data.get("__typename").and_then(Value::as_str)? {
        "Playlist" => playlist_of(data).map(ExploreEntry::Collection),
        "Album" => album_of(data).map(ExploreEntry::Collection),
        _ => None,
    }
}

fn iso_millis(text: &str) -> Option<i64> {
    let mut parts = text.get(..10)?.split('-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
    Some(days_from_civil(year, month, day) * 86_400_000)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}
