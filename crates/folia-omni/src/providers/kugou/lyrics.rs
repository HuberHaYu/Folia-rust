// KuGou lyrics: `search_lyric` for a candidate, `lyric` for its KRC, decoded by XOR and zlib (`decodeLyrics`).

use std::io::Read;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use folia_core::Lyrics;
use folia_lyrics::Format;
use serde_json::Value;

use super::gateway::{APPID, CLIENTVER, Client, Request};
use super::{SongKey, text_of};
use crate::OmniError;

/// `decodeLyrics`' key, used in turn over the bytes after the four-byte `krc1` head.
const KRC_KEY: [u8; 16] = [64, 71, 97, 119, 94, 50, 116, 71, 81, 54, 49, 45, 206, 210, 110, 105];

/// `getKugouLyrics`: word-timed KRC, nothing for an instrumental.
pub async fn lyrics(client: &Client, key: SongKey) -> Result<Option<Lyrics>, OmniError> {
    let found = client
        .call(|| {
            let mut request = Request::get("/v1/search")
                .base("https://lyrics.kugou.com")
                .param("album_audio_id", if key.album_audio_id.is_empty() { "0" } else { &key.album_audio_id })
                .param("appid", APPID)
                .param("clientver", CLIENTVER)
                .param("duration", 0)
                .param("hash", &key.hash)
                .param("keyword", "")
                .param("lrctxt", 1)
                .param("man", "no");
            request.bare = true;
            request
        })
        .await?;
    let Some(candidate) = found.get("candidates").and_then(Value::as_array).and_then(|rows| rows.first()) else {
        return Ok(None);
    };
    let (Some(id), Some(accesskey)) = (text_of(candidate, &["id"]), text_of(candidate, &["accesskey"])) else {
        return Ok(None);
    };
    let answer = client
        .call(|| {
            Request::get("/download")
                .base("https://lyrics.kugou.com")
                .param("ver", 1)
                .param("client", "android")
                .param("id", &id)
                .param("accesskey", &accesskey)
                .param("fmt", "krc")
                .param("charset", "utf8")
        })
        .await?;
    let Some(content) = answer.get("content").and_then(Value::as_str).filter(|content| !content.is_empty()) else {
        return Ok(None);
    };
    let plain = answer.get("contenttype").and_then(Value::as_i64).unwrap_or(0) != 0;
    let text = if plain { decode_base64(content)? } else { decode_krc(content)? };
    let parsed = folia_lyrics::parse(Format::Krc, &text, "", "");
    Ok((!parsed.lines.is_empty() && !is_pure_music(&parsed)).then_some(parsed))
}

fn decode_base64(content: &str) -> Result<String, OmniError> {
    let bytes = STANDARD.decode(content).map_err(|err| OmniError::Response(format!("kugou lyric: {err}")))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// `decodeLyrics`: base64, the head dropped, XOR with the key, then zlib.
fn decode_krc(content: &str) -> Result<String, OmniError> {
    let bytes = STANDARD.decode(content).map_err(|err| OmniError::Response(format!("kugou lyric: {err}")))?;
    let body: Vec<u8> =
        bytes.iter().skip(4).enumerate().map(|(index, byte)| byte ^ KRC_KEY[index % KRC_KEY.len()]).collect();
    let mut text = String::new();
    flate2::read::ZlibDecoder::new(body.as_slice())
        .read_to_string(&mut text)
        .map_err(|err| OmniError::Response(format!("kugou lyric does not inflate: {err}")))?;
    Ok(text)
}

/// `isPureMusicLyricText`: a notice that the song has no words, beside nothing but credit lines.
fn is_pure_music(lyrics: &Lyrics) -> bool {
    let notice = |text: &str| text.contains("纯音乐") || text.contains("没有填词");
    let credit = |text: &str| text.trim().is_empty() || text.contains(':') || text.contains('：');
    let lines = || lyrics.lines.iter().map(|line| line.text.as_str());
    lines().any(notice) && lines().all(|text| notice(text) || credit(text))
}
