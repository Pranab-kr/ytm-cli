//! Metadata cache for instant cold start (NFR-1). Disposable by design:
//! anything wrong with it is fixed by deleting and rebuilding. No audio,
//! no tokens, no secrets (NFR-6).

use crate::model::*;
use rusqlite::{Connection, params};

const SCHEMA_VERSION: i64 = 1;

/// Artists are one column, joined by a byte that cannot occur in a name.
const ARTIST_SEP: char = '\u{1}';

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("cache database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

pub struct Cache {
    conn: Connection,
}

impl Cache {
    pub fn open_in_memory() -> Result<Self, CacheError> {
        let c = Self {
            conn: Connection::open_in_memory()?,
        };
        c.migrate()?;
        Ok(c)
    }

    /// Opens, or rebuilds from scratch if the file is unusable.
    pub fn open(path: &std::path::Path) -> Result<Self, CacheError> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }

        let fresh = |p: &std::path::Path| -> Result<Self, CacheError> {
            let c = Self {
                conn: Connection::open(p)?,
            };
            c.migrate()?;
            Ok(c)
        };

        match fresh(path) {
            Ok(c) => Ok(c),
            Err(e) => {
                // Corrupt or wrong-version file: throw it away and start over.
                tracing::warn!(error = %e, "cache unusable, rebuilding");
                let _ = std::fs::remove_file(path);
                fresh(path)
            }
        }
    }

    fn migrate(&self) -> Result<(), CacheError> {
        self.conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS playlists (
                id TEXT PRIMARY KEY, title TEXT NOT NULL, description TEXT,
                track_count INTEGER, privacy TEXT NOT NULL, thumbnail_url TEXT,
                is_system INTEGER NOT NULL, sort INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS tracks (
                video_id TEXT NOT NULL, set_video_id TEXT, title TEXT NOT NULL,
                artists TEXT NOT NULL, album TEXT, duration INTEGER NOT NULL,
                thumbnail_url TEXT, is_explicit INTEGER NOT NULL,
                playlist_id TEXT, sort INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS tracks_by_playlist ON tracks(playlist_id, sort);",
        )?;
        self.conn.execute(
            "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;
        Ok(())
    }

    pub fn save_playlists(&self, playlists: &[Playlist]) -> Result<(), CacheError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM playlists", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO playlists
                 (id, title, description, track_count, privacy, thumbnail_url, is_system, sort)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            for (i, p) in playlists.iter().enumerate() {
                stmt.execute(params![
                    p.id.as_str(),
                    p.title,
                    p.description,
                    p.track_count,
                    privacy_to_str(p.privacy),
                    p.thumbnail_url,
                    p.is_system as i64,
                    i as i64,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn load_playlists(&self) -> Result<Vec<Playlist>, CacheError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, description, track_count, privacy, thumbnail_url, is_system
             FROM playlists ORDER BY sort",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Playlist {
                id: PlaylistId::from(r.get::<_, String>(0)?),
                title: r.get(1)?,
                description: r.get(2)?,
                track_count: r.get(3)?,
                privacy: privacy_from_str(&r.get::<_, String>(4)?),
                thumbnail_url: r.get(5)?,
                is_system: r.get::<_, i64>(6)? != 0,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Replaces the cached rows for one playlist, leaving other playlists and
    /// the library songs alone.
    pub fn save_playlist_tracks(
        &self,
        playlist_id: &PlaylistId,
        tracks: &[Track],
    ) -> Result<(), CacheError> {
        self.replace_tracks(Some(playlist_id.as_str()), tracks)
    }

    pub fn load_playlist_tracks(&self, playlist_id: &PlaylistId) -> Result<Vec<Track>, CacheError> {
        self.select_tracks("playlist_id = ?1", params![playlist_id.as_str()])
    }

    /// Library songs are the rows with no playlist.
    pub fn save_library_songs(&self, tracks: &[Track]) -> Result<(), CacheError> {
        self.replace_tracks(None, tracks)
    }

    pub fn load_library_songs(&self) -> Result<Vec<Track>, CacheError> {
        self.select_tracks("playlist_id IS NULL", params![])
    }

    pub fn clear(&self) -> Result<(), CacheError> {
        self.conn
            .execute_batch("DELETE FROM playlists; DELETE FROM tracks;")?;
        Ok(())
    }

    fn replace_tracks(
        &self,
        playlist_id: Option<&str>,
        tracks: &[Track],
    ) -> Result<(), CacheError> {
        let tx = self.conn.unchecked_transaction()?;
        match playlist_id {
            Some(id) => tx.execute("DELETE FROM tracks WHERE playlist_id = ?1", params![id])?,
            None => tx.execute("DELETE FROM tracks WHERE playlist_id IS NULL", [])?,
        };
        {
            let mut stmt = tx.prepare(
                "INSERT INTO tracks
                 (video_id, set_video_id, title, artists, album, duration,
                  thumbnail_url, is_explicit, playlist_id, sort)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            for (i, t) in tracks.iter().enumerate() {
                stmt.execute(params![
                    t.video_id.as_str(),
                    t.set_video_id.as_ref().map(|s| s.as_str()),
                    t.title,
                    t.artists.join(&ARTIST_SEP.to_string()),
                    t.album,
                    t.duration.as_secs() as i64,
                    t.thumbnail_url,
                    t.is_explicit as i64,
                    playlist_id,
                    i as i64,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn select_tracks(
        &self,
        where_clause: &str,
        args: impl rusqlite::Params,
    ) -> Result<Vec<Track>, CacheError> {
        let sql = format!(
            "SELECT video_id, set_video_id, title, artists, album, duration,
                    thumbnail_url, is_explicit
             FROM tracks WHERE {where_clause} ORDER BY sort"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(args, |r| {
            let artists: String = r.get(3)?;
            Ok(Track {
                video_id: VideoId::from(r.get::<_, String>(0)?),
                set_video_id: r.get::<_, Option<String>>(1)?.map(SetVideoId::from),
                title: r.get(2)?,
                artists: split_artists(&artists),
                album: r.get(4)?,
                duration: TrackDuration::from_secs(r.get::<_, i64>(5)?.max(0) as u64),
                thumbnail_url: r.get(6)?,
                is_explicit: r.get::<_, i64>(7)? != 0,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

fn privacy_to_str(p: Privacy) -> &'static str {
    match p {
        Privacy::Private => "private",
        Privacy::Public => "public",
        Privacy::Unlisted => "unlisted",
    }
}

/// Unknown values load as `Private` — a cache row is never worth failing over,
/// and private is the safe assumption about someone's playlist.
fn privacy_from_str(s: &str) -> Privacy {
    match s {
        "public" => Privacy::Public,
        "unlisted" => Privacy::Unlisted,
        _ => Privacy::Private,
    }
}

fn split_artists(joined: &str) -> Vec<String> {
    if joined.is_empty() {
        Vec::new()
    } else {
        joined.split(ARTIST_SEP).map(|s| s.to_owned()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_in_memory_and_starts_empty() {
        let c = Cache::open_in_memory().unwrap();
        assert!(c.load_playlists().unwrap().is_empty());
    }

    #[test]
    fn playlists_round_trip() {
        let c = Cache::open_in_memory().unwrap();
        c.save_playlists(&[
            Playlist {
                track_count: Some(12),
                ..Playlist::stub("p1", "Focus")
            },
            Playlist::stub("p2", "Chill"),
        ])
        .unwrap();
        let got = c.load_playlists().unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].title, "Focus");
        assert_eq!(got[0].track_count, Some(12));
    }

    #[test]
    fn saving_replaces_rather_than_appends() {
        // A refresh must not duplicate rows.
        let c = Cache::open_in_memory().unwrap();
        c.save_playlists(&[Playlist::stub("p1", "A")]).unwrap();
        c.save_playlists(&[Playlist::stub("p1", "A renamed")])
            .unwrap();
        let got = c.load_playlists().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].title, "A renamed");
    }

    #[test]
    fn playlist_tracks_round_trip_preserving_order_and_set_video_id() {
        let c = Cache::open_in_memory().unwrap();
        let id = PlaylistId::from("p1");
        let tracks = vec![
            Track {
                set_video_id: Some(SetVideoId::from("s1")),
                ..Track::stub("v1", "First")
            },
            Track {
                set_video_id: Some(SetVideoId::from("s2")),
                ..Track::stub("v2", "Second")
            },
        ];
        c.save_playlist_tracks(&id, &tracks).unwrap();
        let got = c.load_playlist_tracks(&id).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].title, "First", "order must survive the round trip");
        assert_eq!(
            got[1].set_video_id,
            Some(SetVideoId::from("s2")),
            "removal depends on this"
        );
    }

    #[test]
    fn tracks_for_an_unknown_playlist_are_empty_not_an_error() {
        let c = Cache::open_in_memory().unwrap();
        assert!(
            c.load_playlist_tracks(&PlaylistId::from("nope"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn multiple_artists_survive_the_round_trip() {
        let c = Cache::open_in_memory().unwrap();
        let t = Track {
            artists: vec!["A".into(), "B".into()],
            ..Track::stub("v1", "T")
        };
        c.save_library_songs(&[t]).unwrap();
        assert_eq!(c.load_library_songs().unwrap()[0].artists, vec!["A", "B"]);
    }

    #[test]
    fn clear_empties_every_table() {
        let c = Cache::open_in_memory().unwrap();
        c.save_playlists(&[Playlist::stub("p1", "A")]).unwrap();
        c.save_library_songs(&[Track::stub("v1", "T")]).unwrap();
        c.clear().unwrap();
        assert!(c.load_playlists().unwrap().is_empty());
        assert!(c.load_library_songs().unwrap().is_empty());
    }

    #[test]
    fn a_corrupt_database_file_is_rebuilt_rather_than_fatal() {
        // The cache is disposable; a bad file must never block startup.
        let dir = std::env::temp_dir().join(format!("ytmcache{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("cache.db");
        std::fs::write(&path, b"this is not a database").unwrap();
        let c = Cache::open(&path).expect("must recover, not fail");
        assert!(c.load_playlists().unwrap().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
