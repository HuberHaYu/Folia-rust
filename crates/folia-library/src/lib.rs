// Local music library: watched folders, an incremental scan into SQLite, and lyric lookup.

mod lyrics;
mod tags;

pub use lyrics::{decode_text, lyrics_for};

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use folia_core::Track;
use rayon::prelude::*;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

const SCHEMA_VERSION: i32 = 1;
const SCHEMA: &str = "
CREATE TABLE folders (path TEXT PRIMARY KEY);
CREATE TABLE tracks (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    artist TEXT,
    album TEXT,
    album_artist TEXT,
    track_no INTEGER,
    disc_no INTEGER,
    duration REAL NOT NULL,
    size INTEGER NOT NULL,
    mtime INTEGER NOT NULL
);";

/// Audio file extensions the library picks up, as in the original app.
pub const AUDIO_EXTENSIONS: [&str; 16] =
    ["mp3", "flac", "m4a", "wav", "ogg", "opus", "aac", "alac", "ape", "wv", "tta", "wma", "aif", "aiff", "caf", "dsf"];

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("library database error")]
    Db(#[from] rusqlite::Error),
    #[error("cannot create {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("path is not valid UTF-8: {0}")]
    NonUtf8Path(PathBuf),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanReport {
    pub updated: usize,
    pub removed: usize,
    pub total: usize,
}

pub struct Library {
    conn: Connection,
}

struct FoundFile {
    path: PathBuf,
    key: String,
    size: i64,
    mtime: i64,
}

impl Library {
    /// Opens or creates the database; an index from another schema version is dropped and rebuilt by the next scan.
    pub fn open(db_path: &Path) -> Result<Self, LibraryError> {
        if let Some(dir) = db_path.parent() {
            std::fs::create_dir_all(dir).map_err(|source| LibraryError::Io { path: dir.to_owned(), source })?;
        }
        let conn = Connection::open(db_path)?;
        let folders = Self::init(&conn)?;
        let library = Self { conn };
        for folder in folders {
            library.conn.execute("INSERT OR IGNORE INTO folders (path) VALUES (?1)", [folder])?;
        }
        Ok(library)
    }

    /// Creates the schema when needed and returns the folders to keep across a rebuild.
    fn init(conn: &Connection) -> Result<Vec<String>, LibraryError> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let version: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version == SCHEMA_VERSION {
            return Ok(Vec::new());
        }
        let has_folders: bool = conn
            .query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'folders'", [], |_| Ok(true))
            .optional()?
            .unwrap_or(false);
        let folders = if has_folders { query_strings(conn, "SELECT path FROM folders")? } else { Vec::new() };
        if version != 0 {
            tracing::info!(from = version, to = SCHEMA_VERSION, "rebuilding library index");
        }
        conn.execute_batch("DROP TABLE IF EXISTS folders; DROP TABLE IF EXISTS tracks;")?;
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(folders)
    }

    pub fn folders(&self) -> Result<Vec<PathBuf>, LibraryError> {
        Ok(query_strings(&self.conn, "SELECT path FROM folders ORDER BY path")?
            .into_iter()
            .map(PathBuf::from)
            .collect())
    }

    pub fn add_folder(&self, folder: &Path) -> Result<(), LibraryError> {
        self.conn.execute("INSERT OR IGNORE INTO folders (path) VALUES (?1)", [utf8(folder)?])?;
        Ok(())
    }

    /// Forgets a folder and every track that only it covered.
    pub fn remove_folder(&mut self, folder: &Path) -> Result<usize, LibraryError> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM folders WHERE path = ?1", [utf8(folder)?])?;
        let remaining = query_strings(&tx, "SELECT path FROM folders")?;
        let orphans: Vec<String> = query_strings(&tx, "SELECT path FROM tracks")?
            .into_iter()
            .filter(|path| !remaining.iter().any(|root| Path::new(path).starts_with(root)))
            .collect();
        let removed = delete_tracks(&tx, &orphans)?;
        tx.commit()?;
        Ok(removed)
    }

    /// Walks every folder, re-reads files whose size or modification time changed, and drops vanished files.
    pub fn scan(&mut self) -> Result<ScanReport, LibraryError> {
        let found = self.find_files()?;
        let known: HashMap<String, (i64, i64)> = {
            let mut stmt = self.conn.prepare("SELECT path, size, mtime FROM tracks")?;
            stmt.query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?))))?.collect::<Result<_, _>>()?
        };

        let changed: Vec<(&FoundFile, Track)> = found
            .par_iter()
            .filter(|file| known.get(&file.key) != Some(&(file.size, file.mtime)))
            .map(|file| (file, tags::read(&file.path)))
            .collect();
        let found_keys: HashSet<&str> = found.iter().map(|file| file.key.as_str()).collect();
        let vanished: Vec<String> = known.into_keys().filter(|key| !found_keys.contains(key.as_str())).collect();

        let tx = self.conn.transaction()?;
        {
            let mut upsert = tx.prepare(
                "INSERT INTO tracks (path, title, artist, album, album_artist, track_no, disc_no, duration, size, mtime)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(path) DO UPDATE SET title = ?2, artist = ?3, album = ?4, album_artist = ?5,
                 track_no = ?6, disc_no = ?7, duration = ?8, size = ?9, mtime = ?10",
            )?;
            for (file, t) in &changed {
                upsert.execute(params![
                    file.key,
                    t.title,
                    t.artist,
                    t.album,
                    t.album_artist,
                    t.track_no,
                    t.disc_no,
                    t.duration,
                    file.size,
                    file.mtime
                ])?;
            }
        }
        let removed = delete_tracks(&tx, &vanished)?;
        tx.commit()?;

        let report = ScanReport { updated: changed.len(), removed, total: found.len() };
        tracing::info!(updated = report.updated, removed = report.removed, total = report.total, "library scanned");
        Ok(report)
    }

    /// All tracks, ordered by artist, album, disc, track number and title.
    pub fn tracks(&self) -> Result<Vec<Track>, LibraryError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, path, title, artist, album, album_artist, track_no, disc_no, duration FROM tracks
             ORDER BY COALESCE(album_artist, artist) COLLATE NOCASE, album COLLATE NOCASE,
             disc_no, track_no, title COLLATE NOCASE",
        )?;
        let tracks = stmt.query_map([], |row| {
            Ok(Track {
                id: row.get(0)?,
                path: PathBuf::from(row.get::<_, String>(1)?),
                title: row.get(2)?,
                artist: row.get(3)?,
                album: row.get(4)?,
                album_artist: row.get(5)?,
                track_no: row.get(6)?,
                disc_no: row.get(7)?,
                duration: row.get(8)?,
            })
        })?;
        Ok(tracks.collect::<Result<_, _>>()?)
    }

    fn find_files(&self) -> Result<Vec<FoundFile>, LibraryError> {
        let mut seen = HashSet::new();
        let mut files = Vec::new();
        for folder in self.folders()? {
            for entry in walkdir::WalkDir::new(&folder).follow_links(true).into_iter().filter_map(Result::ok) {
                if !entry.file_type().is_file() || !is_audio(entry.path()) {
                    continue;
                }
                let Some(key) = entry.path().to_str().map(str::to_owned) else {
                    tracing::warn!(path = %entry.path().display(), "skipping non UTF-8 path");
                    continue;
                };
                let Ok(meta) = entry.metadata() else { continue };
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |since| since.as_secs() as i64);
                if seen.insert(key.clone()) {
                    files.push(FoundFile { path: entry.into_path(), key, size: meta.len() as i64, mtime });
                }
            }
        }
        Ok(files)
    }
}

pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| AUDIO_EXTENSIONS.iter().any(|known| known.eq_ignore_ascii_case(ext)))
}

fn delete_tracks(tx: &Transaction<'_>, paths: &[String]) -> Result<usize, LibraryError> {
    let mut stmt = tx.prepare("DELETE FROM tracks WHERE path = ?1")?;
    let mut removed = 0;
    for path in paths {
        removed += stmt.execute([path])?;
    }
    Ok(removed)
}

fn query_strings(conn: &Connection, sql: &str) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt = conn.prepare(sql)?;
    stmt.query_map([], |row| row.get(0))?.collect()
}

fn utf8(path: &Path) -> Result<&str, LibraryError> {
    path.to_str().ok_or_else(|| LibraryError::NonUtf8Path(path.to_owned()))
}
