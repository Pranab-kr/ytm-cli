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
    /// A Netscape cookie jar for yt-dlp, if cookie auth is configured.
    ///
    /// YouTube now answers anonymous stream requests with "Sign in to confirm
    /// you're not a bot", so playback fails outright without cookies. We already
    /// hold cookies for the API, but in a different format — see
    /// `netscape_from_header`.
    cookie_jar: Mutex<Option<std::path::PathBuf>>,
}

/// Convert a raw `Cookie:` header into the Netscape jar format yt-dlp wants.
///
/// The two formats are not interchangeable and this is the whole reason a
/// conversion exists: `ytmapi-rs`'s `BrowserToken` takes the header string
/// verbatim, while yt-dlp's `--cookies` reads a tab-separated jar. Rather than
/// asking the user to export cookies twice, we derive one from the other.
///
/// Returns `None` when the input carries no cookies at all, so a malformed file
/// leaves playback unauthenticated rather than handing yt-dlp an empty jar.
pub fn netscape_from_header(header: &str) -> Option<String> {
    // Far-future expiry: these are session cookies as far as we can tell from a
    // header, which carries no expiry, and a past date would make yt-dlp discard
    // every line.
    const EXPIRY: &str = "2147483647";
    let mut out = String::from("# Netscape HTTP Cookie File\n");
    let mut any = false;
    for pair in header.split(';') {
        let pair = pair.trim();
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        // domain, include-subdomains, path, secure, expiry, name, value — tabs,
        // not spaces; yt-dlp rejects the line otherwise.
        out.push_str(&format!(
            ".youtube.com\tTRUE\t/\tTRUE\t{EXPIRY}\t{name}\t{}\n",
            value.trim()
        ));
        any = true;
    }
    any.then_some(out)
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
            cookie_jar: Mutex::new(None),
        }
    }

    /// Point yt-dlp at cookies derived from the API's cookie file.
    ///
    /// The jar is written once to a temp file and reused: converting per resolve
    /// would rewrite it on every track. Failure is deliberately silent — losing
    /// cookies degrades to the unauthenticated path, which yt-dlp reports itself
    /// with a message the user can act on.
    pub fn use_cookie_header_file(&self, path: &std::path::Path) {
        let Ok(header) = std::fs::read_to_string(path) else {
            return;
        };
        let Some(jar) = netscape_from_header(&header) else {
            return;
        };
        let out = std::env::temp_dir().join(format!("ytm-cli-cookies-{}.txt", std::process::id()));
        if std::fs::write(&out, jar).is_ok() {
            // Readable only by this user: it holds live session cookies.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o600));
            }
            *self.cookie_jar.lock().unwrap() = Some(out);
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
        let jar = self.cookie_jar.lock().unwrap().clone();
        let url = Self::run_yt_dlp(id, jar.as_deref()).await?;
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
    async fn run_yt_dlp(
        id: &VideoId,
        cookies: Option<&std::path::Path>,
    ) -> Result<String, ResolveError> {
        let url = format!("https://music.youtube.com/watch?v={id}");
        let mut cmd = tokio::process::Command::new("yt-dlp");
        cmd.args(["-f", "bestaudio", "--no-playlist", "--no-warnings", "-g"]);
        // Without cookies YouTube answers "Sign in to confirm you're not a bot"
        // and nothing plays.
        if let Some(jar) = cookies {
            cmd.arg("--cookies").arg(jar);
        }
        let out = cmd.arg(&url).output().await.map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ResolveError::NotInstalled,
            _ => ResolveError::Failed(e.to_string()),
        })?;

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            // yt-dlp's own wording ("Sign in to confirm you're not a bot") sends
            // the user to browser cookies, which is not what this app needs — it
            // needs auth.cookie_file, which it may already have.
            if stderr.contains("not a bot") || stderr.contains("Sign in to confirm") {
                return Err(ResolveError::Failed(
                    "YouTube requires cookies to stream — set auth.cookie_file in \
                     config.toml (see the README) and restart"
                        .to_owned(),
                ));
            }
            return Err(ResolveError::Failed(
                stderr.lines().last().unwrap_or("unknown").to_owned(),
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

    #[test]
    fn a_cookie_header_becomes_a_netscape_jar() {
        // The two formats are not interchangeable: ytmapi-rs takes the raw
        // header, yt-dlp takes a tab-separated jar. Without this conversion
        // YouTube answers every stream request with its bot check.
        let jar = netscape_from_header("SAPISID=abc; HSID=def; SSID=ghi").expect("should convert");
        assert!(jar.starts_with("# Netscape HTTP Cookie File"));
        let lines: Vec<&str> = jar.lines().skip(1).collect();
        assert_eq!(lines.len(), 3);
        // Tabs, not spaces — yt-dlp silently rejects a space-separated line.
        for line in &lines {
            assert_eq!(
                line.matches('\t').count(),
                6,
                "7 tab-separated fields: {line}"
            );
            assert!(line.starts_with(".youtube.com\tTRUE\t/\tTRUE\t"));
        }
        assert!(lines[0].ends_with("\tSAPISID\tabc"));
    }

    #[test]
    fn a_cookie_value_containing_an_equals_sign_survives() {
        // Base64-ish cookie values carry '=' padding; splitting on every '='
        // would truncate them and the cookie would be rejected.
        let jar = netscape_from_header("PSID=a=b=c").expect("should convert");
        assert!(jar.lines().nth(1).unwrap().ends_with("\tPSID\ta=b=c"));
    }

    #[test]
    fn whitespace_and_stray_semicolons_do_not_produce_junk_lines() {
        let jar = netscape_from_header("  A=1 ;; B=2 ; ").expect("should convert");
        assert_eq!(jar.lines().count(), 3, "header + two cookies");
    }

    #[test]
    fn a_header_with_no_cookies_converts_to_nothing() {
        // Returning an empty jar would hand yt-dlp a file that authenticates
        // nothing while looking like it should.
        assert!(netscape_from_header("").is_none());
        assert!(netscape_from_header("not a cookie header").is_none());
        assert!(netscape_from_header(";;;").is_none());
    }

    #[test]
    fn a_resolver_without_cookies_still_works() {
        // Cookie auth is optional: OAuth users have no cookie file, and the
        // resolver must not require one to exist.
        let r = StreamResolver::new();
        r.use_cookie_header_file(std::path::Path::new("/nonexistent/cookies.txt"));
        assert!(r.cookie_jar.lock().unwrap().is_none());
    }
}
