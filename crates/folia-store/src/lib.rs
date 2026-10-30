// Settings persistence: each domain is one versioned TOML file in the config directory.

mod general;

pub use general::GeneralSettings;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

const VERSION_KEY: &str = "schema_version";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("no config directory is available on this system")]
    NoConfigDir,
    #[error("cannot access {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("cannot parse {path}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("cannot serialize {file}")]
    Serialize {
        file: &'static str,
        #[source]
        source: toml::ser::Error,
    },
    #[error("{file} has schema version {found}, expected {expected}")]
    SchemaVersion { file: &'static str, found: u32, expected: u32 },
}

/// A group of settings stored in its own file; bump `SCHEMA_VERSION` whenever the layout changes.
pub trait Domain: Serialize + DeserializeOwned + Default {
    const FILE: &'static str;
    const SCHEMA_VERSION: u32;
}

#[derive(Deserialize)]
struct Header {
    schema_version: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// Opens the store in the platform config directory of `app_name`.
    pub fn open(app_name: &str) -> Result<Self, StoreError> {
        let dirs = ProjectDirs::from("", "", app_name).ok_or(StoreError::NoConfigDir)?;
        Ok(Self::at(dirs.config_dir()))
    }

    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Reads a domain, returning its default when the file does not exist yet.
    pub fn load<T: Domain>(&self) -> Result<T, StoreError> {
        let path = self.dir.join(T::FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(T::default()),
            Err(source) => return Err(StoreError::Io { path, source }),
        };

        let parse_err = |source| StoreError::Parse { path: path.clone(), source };
        let found = toml::from_str::<Header>(&text).map_err(parse_err)?.schema_version.unwrap_or(0);
        if found != T::SCHEMA_VERSION {
            return Err(StoreError::SchemaVersion { file: T::FILE, found, expected: T::SCHEMA_VERSION });
        }
        toml::from_str(&text).map_err(parse_err)
    }

    /// Writes a domain atomically through a temporary file.
    pub fn save<T: Domain>(&self, value: &T) -> Result<(), StoreError> {
        let body = toml::to_string(value).map_err(|source| StoreError::Serialize { file: T::FILE, source })?;
        let text = format!("{VERSION_KEY} = {}\n{body}", T::SCHEMA_VERSION);

        let path = self.dir.join(T::FILE);
        let tmp = path.with_extension("toml.tmp");
        let io_err = |source| StoreError::Io { path: path.clone(), source };
        fs::create_dir_all(&self.dir).map_err(io_err)?;
        fs::write(&tmp, text).map_err(io_err)?;
        fs::rename(&tmp, &path).map_err(io_err)
    }
}
