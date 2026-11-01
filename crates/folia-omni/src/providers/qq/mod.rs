// QQ Music: anonymous musicu search and lyrics, Android musicu with the QQ Connect credential for the library.

mod library;
mod login;
mod musicu;
mod qrc;

use folia_core::{Lyrics, OnlineId, Track};
use serde_json::Value;

use crate::{
    Account, AudioSource, BoxFuture, Collection, CollectionDetail, CollectionKind, ExploreSection, LoginCode,
    LoginState, OmniError, Page, Provider, RadioFeed,
};

const ID: &str = "qq";
const ALBUM_COVER: &str = "https://y.gtimg.cn/music/photo_new/T002M000";

pub struct Qq {
    client: musicu::Client,
    /// The sign-in chain reads its redirects itself (`redirect: 'manual'`).
    plain: reqwest::Client,
    /// Keywords whose signed-in first page came back empty: later pages search anonymously too.
    anonymous: std::sync::Mutex<std::collections::VecDeque<String>>,
}

impl Qq {
    pub fn new(http: reqwest::Client) -> Self {
        let plain = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36")
            .build()
            .unwrap_or_else(|err| {
                tracing::warn!(error = &err as &dyn std::error::Error, "qq sign-in client falls back to the shared one");
                http.clone()
            });
        Self { client: musicu::Client::new(http), plain, anonymous: std::sync::Mutex::default() }
    }
}

impl Provider for Qq {
    fn id(&self) -> &'static str {
        ID
    }

    fn display_name(&self) -> &'static str {
        "QQ音乐"
    }

    fn search_songs<'a>(
        &'a self,
        query: &'a str,
        limit: u32,
        offset: u32,
    ) -> BoxFuture<'a, Result<Page<Track>, OmniError>> {
        Box::pin(library::search(&self.client, &self.anonymous, query, limit, offset))
    }

    fn audio_source<'a>(&'a self, song: &'a OnlineId) -> BoxFuture<'a, Result<AudioSource, OmniError>> {
        Box::pin(library::audio_source(&self.client, SongKey::parse(&song.id)))
    }

    fn lyrics<'a>(&'a self, song: &'a OnlineId) -> BoxFuture<'a, Result<Option<Lyrics>, OmniError>> {
        Box::pin(library::lyrics(&self.client, SongKey::parse(&song.id)))
    }

    fn login_code(&self) -> BoxFuture<'_, Result<LoginCode, OmniError>> {
        Box::pin(login::login_code(&self.client, &self.plain))
    }

    fn check_login<'a>(&'a self, key: &'a str) -> BoxFuture<'a, Result<LoginState, OmniError>> {
        Box::pin(login::check_login(&self.client, &self.plain, key))
    }

    fn account(&self) -> BoxFuture<'_, Result<Option<Account>, OmniError>> {
        Box::pin(library::account(&self.client))
    }

    /// `logout`: the package only forgets its session; nothing is sent to QQ.
    fn logout(&self) -> BoxFuture<'_, Result<(), OmniError>> {
        Box::pin(async move {
            self.client.set_credential(None);
            self.anonymous.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
            Ok(())
        })
    }

    fn session(&self) -> Option<String> {
        let session = self.client.session();
        session.credential.as_ref()?;
        serde_json::to_string(&session).ok()
    }

    fn restore_session(&self, saved: &str) -> Result<(), OmniError> {
        let session: musicu::Session =
            serde_json::from_str(saved).map_err(|err| OmniError::Response(format!("kept session: {err}")))?;
        self.client.set_session(session);
        Ok(())
    }

    fn user_playlists<'a>(&'a self, _user: &'a str) -> BoxFuture<'a, Result<Vec<Collection>, OmniError>> {
        Box::pin(library::user_playlists(&self.client))
    }

    fn collection_detail<'a>(
        &'a self,
        collection: &'a Collection,
    ) -> BoxFuture<'a, Result<CollectionDetail, OmniError>> {
        Box::pin(library::collection_detail(&self.client, collection))
    }

    fn set_subscribed<'a>(
        &'a self,
        collection: &'a Collection,
        subscribed: bool,
    ) -> BoxFuture<'a, Result<(), OmniError>> {
        Box::pin(library::set_subscribed(&self.client, collection, subscribed))
    }

    fn favorite_albums(&self) -> BoxFuture<'_, Result<Vec<Collection>, OmniError>> {
        Box::pin(library::favorite_albums(&self.client))
    }

    fn radio_feed(&self, limit: u32) -> BoxFuture<'_, Result<RadioFeed, OmniError>> {
        Box::pin(library::radio_feed(&self.client, limit))
    }

    fn explore(&self) -> BoxFuture<'_, Result<Vec<ExploreSection>, OmniError>> {
        Box::pin(library::explore(&self.client))
    }

    /// `withQqCoverSize`: `T00xR<n>x<n>M000` on `y.gtimg.cn`, 300 or 800, else the original.
    fn sized_cover(&self, url: &str, px: u32) -> Option<String> {
        let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
        let host = rest.split('/').next()?;
        if host != "y.gtimg.cn" && host != "y.qq.com" {
            return None;
        }
        let at = url.find("T001").or_else(|| url.find("T002"))?;
        let tail = &url[at + 4..];
        let after = if let Some(sized) = tail.strip_prefix('R') {
            let end = sized.find("M000")?;
            &sized[end..]
        } else {
            tail
        };
        if !after.starts_with("M000") {
            return None;
        }
        let size = if px <= 300 {
            Some(300)
        } else if px <= 800 {
            Some(800)
        } else {
            None
        };
        let sized = size.map(|size| format!("R{size}x{size}")).unwrap_or_default();
        Some(format!("{}{sized}{after}", &url[..at + 4]))
    }
}

/// What a QQ song id carries: the songmid that plays, the numeric id lyrics need, and the file's media mid.
pub(super) struct SongKey {
    mid: String,
    id: String,
    media_mid: String,
}

impl SongKey {
    /// `songmid-songid-mediamid`, the last two possibly empty.
    fn parse(id: &str) -> Self {
        let mut parts = id.split('-');
        let mut next = || parts.next().unwrap_or_default().to_owned();
        Self { mid: next(), id: next(), media_mid: next() }
    }
}

fn pick<'a>(raw: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| raw.get(*key).filter(|value| !value.is_null()))
}

fn text(raw: &Value, keys: &[&str]) -> Option<String> {
    match pick(raw, keys)? {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_owned()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// `getOriginalCoverUrl` for a QQ cover: the size in `T00xR…M000` dropped.
fn original_cover(url: &str) -> String {
    let secure = url.strip_prefix("http://").map(|rest| format!("https://{rest}")).unwrap_or_else(|| url.to_owned());
    match (secure.find("T001R").or_else(|| secure.find("T002R")), secure.find("M000")) {
        (Some(at), Some(end)) if end > at => format!("{}{}", &secure[..at + 4], &secure[end..]),
        _ => secure,
    }
}

fn album_cover(album_mid: &str) -> Option<String> {
    (!album_mid.is_empty()).then(|| format!("{ALBUM_COVER}{album_mid}.jpg?max_age=2592000"))
}

/// `normalizeQqSong` for the new (`mid`, `album`, `file`) and the old (`songmid`, `albummid`) shapes.
fn track(raw: &Value) -> Option<Track> {
    let wrapped =
        |key: &&str| raw.get(*key).filter(|inner| inner.get("mid").or_else(|| inner.get("songmid")).is_some());
    let raw = ["songInfo", "track", "data", "musicData"].iter().find_map(wrapped).unwrap_or(raw);
    let album = raw.get("album").unwrap_or(&Value::Null);
    let file = raw.get("file").unwrap_or(&Value::Null);
    let mid = text(raw, &["mid", "songmid", "songMid"])?;
    let id =
        text(raw, &["id", "songid", "songId"]).filter(|id| id.bytes().all(|b| b.is_ascii_digit())).unwrap_or_default();
    let album_mid = text(album, &["mid", "albummid", "albumMid"])
        .or_else(|| text(raw, &["albummid", "albumMid"]))
        .unwrap_or_default();
    let media_mid = text(file, &["media_mid", "mediaMid"])
        .or_else(|| text(raw, &["strMediaMid", "mediaMid", "media_mid"]))
        .unwrap_or_default();
    let artists: Vec<String> = pick(raw, &["singer", "singers", "artists"])
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(|singer| text(singer, &["name", "title", "singername"])).collect())
        .unwrap_or_default();
    let cover =
        text(album, &["coverUrl", "picUrl"]).map(|url| original_cover(&url)).or_else(|| album_cover(&album_mid));
    let online = OnlineId { provider: ID.to_owned(), id: format!("{mid}-{id}-{media_mid}") };
    Some(Track {
        id: online.track_id(),
        title: text(raw, &["title", "name", "songname"]).unwrap_or_else(|| "Unknown Song".to_owned()),
        artist: (!artists.is_empty()).then(|| artists.join(" / ")),
        album: text(album, &["name", "title", "albumname"]).or_else(|| text(raw, &["albumname", "albumName"])),
        cover_url: cover,
        duration: pick(raw, &["interval", "duration"]).and_then(Value::as_f64).unwrap_or(0.0),
        online: Some(online),
        ..Track::default()
    })
}

fn tracks(list: Option<&Value>) -> Vec<Track> {
    let mut seen = std::collections::HashSet::new();
    list.and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(track).filter(|track| seen.insert(track.online.clone())).collect())
        .unwrap_or_default()
}

/// `LIKED_SONGS_DIRID`: the owned 「我喜欢」, read by its directory.
const LIKED_DIR: &str = "201";

/// `normalizeQqCollection` for a playlist: `tid` or `dissid`, else (and for the owned liked list) `dir-<id>`.
fn playlist(raw: &Value) -> Option<Collection> {
    let tid = text(raw, &["tid", "dissid", "disstid"]).filter(|id| id != "0");
    let dir = text(raw, &["dirId", "dirid"]);
    let liked = raw.get("dirName").is_some() && dir.as_deref() == Some(LIKED_DIR);
    let tid = tid.filter(|_| !liked);
    let id = tid.or_else(|| dir.map(|dir| format!("dir-{dir}"))).or_else(|| text(raw, &["id"]))?;
    let creator = raw
        .get("creator")
        .and_then(|creator| text(creator, &["nick", "nickname", "name"]))
        .or_else(|| text(raw, &["nick", "nickname"]));
    let cover =
        text(raw, &["bigpicUrl", "picUrl", "picurl", "coverUrl", "logo", "imgurl"]).map(|url| original_cover(&url));
    Some(Collection {
        provider: ID.to_owned(),
        id,
        kind: CollectionKind::Playlist,
        name: text(raw, &["dirName", "dirname", "dissname", "title", "name"])?,
        cover_url: cover,
        track_count: pick(raw, &["songNum", "songnum", "song_cnt", "trackCount"])
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
        creator,
        description: text(raw, &["desc", "description"]),
    })
}

/// `normalizeQqAlbum`: by its mid.
fn album(raw: &Value) -> Option<Collection> {
    let mid = text(raw, &["albummid", "albumMid", "albumMID", "mid"])?;
    let singers: Vec<String> = pick(raw, &["singer", "singers", "singer_list"])
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(|singer| text(singer, &["name", "singername", "title"])).collect())
        .unwrap_or_default();
    let creator = if singers.is_empty() { text(raw, &["singername", "singerName"]) } else { Some(singers.join(", ")) };
    let cover = text(raw, &["coverUrl", "picUrl", "albumPic", "pic"])
        .map(|url| original_cover(&url))
        .or_else(|| album_cover(&mid));
    Some(Collection {
        provider: ID.to_owned(),
        id: mid,
        kind: CollectionKind::Album,
        name: text(raw, &["albumname", "albumName", "name", "title"])?,
        cover_url: cover,
        track_count: pick(raw, &["songnum", "totalNum", "song_count", "total_song_num"])
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok()),
        creator,
        description: text(raw, &["desc", "description"]),
    })
}
