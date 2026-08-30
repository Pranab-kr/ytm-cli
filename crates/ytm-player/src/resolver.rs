//! Turns a VideoId into a playable audio URL by shelling out to yt-dlp.
//!
//! Isolated on purpose: yt-dlp breaking is the single most likely runtime
//! failure, and this is the only file that needs to change when it does.

use std::collections::HashMap;
use std::sync::Mutex;
use ytm_core::VideoId;

/// Resolved URLs are valid ~6h upstream; expire at 4h so playback never
/// starts with a URL that dies mid-track.
pub const TTL_SECS: i64 = 4 * 60 * 60;

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("yt-dlp is not installed or not on PATH — install it to play audio")]
    NotInstalled,
    #[error("yt-dlp could not find an audio stream for this track")]
    NoAudioStream,
    #[error("yt-dlp failed: {0}")]
    Failed(String),
}

struct Entry {
    url: String,
    fetched_at: i64,
}

pub struct StreamResolver {
    cache: Mutex<HashMap<VideoId, Entry>>,
}

impl Default for StreamResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamResolver {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Cached URL if present and still inside the TTL.
    pub fn cached_at(&self, id: &VideoId, now_unix: i64) -> Option<String> {
        let g = self.cache.lock().unwrap();
        let e = g.get(id)?;
        (now_unix - e.fetched_at < TTL_SECS).then(|| e.url.clone())
    }

    pub fn invalidate(&self, id: &VideoId) {
        self.cache.lock().unwrap().remove(id);
    }

    #[doc(hidden)]
    pub fn insert_for_test(&self, id: &VideoId, url: &str, at: i64) {
        self.cache.lock().unwrap().insert(
            id.clone(),
            Entry {
                url: url.to_owned(),
                fetched_at: at,
            },
        );
    }

    /// Resolve, using the cache when warm.
    pub async fn resolve(&self, id: &VideoId) -> Result<String, ResolveError> {
        let now = now_unix();
        if let Some(u) = self.cached_at(id, now) {
            return Ok(u);
        }
        let url = Self::run_yt_dlp(id).await?;
        self.cache.lock().unwrap().insert(
            id.clone(),
            Entry {
                url: url.clone(),
                fetched_at: now,
            },
        );
        Ok(url)
    }

    /// `-g` prints the direct URL; `-f bestaudio` avoids downloading video.
    async fn run_yt_dlp(id: &VideoId) -> Result<String, ResolveError> {
        let url = format!("https://music.youtube.com/watch?v={id}");
        let out = tokio::process::Command::new("yt-dlp")
            .args([
                "-f",
                "bestaudio",
                "--no-playlist",
                "--no-warnings",
                "-g",
                &url,
            ])
            .output()
            .await
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => ResolveError::NotInstalled,
                _ => ResolveError::Failed(e.to_string()),
            })?;

        if !out.status.success() {
            return Err(ResolveError::Failed(
                String::from_utf8_lossy(&out.stderr)
                    .lines()
                    .last()
                    .unwrap_or("unknown")
                    .to_owned(),
            ));
        }
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .find(|l| l.starts_with("http"))
            .map(str::to_owned)
            .ok_or(ResolveError::NoAudioStream)
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ytm_core::VideoId;

    #[test]
    fn cache_returns_a_fresh_entry() {
        let r = StreamResolver::new();
        r.insert_for_test(&VideoId::from("v1"), "https://example.com/a", 1_000);
        assert_eq!(
            r.cached_at(&VideoId::from("v1"), 1_100).as_deref(),
            Some("https://example.com/a")
        );
    }

    #[test]
    fn cache_expires_after_the_ttl() {
        // Google's URLs die around 6h; we expire at 4h for margin.
        let r = StreamResolver::new();
        r.insert_for_test(&VideoId::from("v1"), "https://example.com/a", 1_000);
        assert!(
            r.cached_at(&VideoId::from("v1"), 1_000 + TTL_SECS - 1)
                .is_some()
        );
        assert!(
            r.cached_at(&VideoId::from("v1"), 1_000 + TTL_SECS + 1)
                .is_none()
        );
    }

    #[test]
    fn invalidate_drops_the_entry_so_a_403_can_re_resolve() {
        // FR-P6: a stale URL must be evicted before the retry.
        let r = StreamResolver::new();
        r.insert_for_test(&VideoId::from("v1"), "https://example.com/a", 1_000);
        r.invalidate(&VideoId::from("v1"));
        assert!(r.cached_at(&VideoId::from("v1"), 1_001).is_none());
    }

    #[test]
    fn ttl_is_four_hours() {
        assert_eq!(TTL_SECS, 4 * 60 * 60);
    }

    #[tokio::test]
    #[ignore = "hits the network via yt-dlp; run manually with --ignored"]
    async fn resolves_a_real_video_to_an_https_url() {
        let r = StreamResolver::new();
        let url = r.resolve(&VideoId::from("dQw4w9WgXcQ")).await.unwrap();
        assert!(url.starts_with("https://"), "got: {url}");
    }
}
