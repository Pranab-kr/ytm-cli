//! Optimistic edits (FR-C6). Every mutation carries enough state to undo
//! itself, so a late failure reverts exactly its own change and nothing else.
//!
//! The token is what makes that work. Two edits can be in flight at once, and
//! the responses can arrive in either order, so a failure has to name the edit
//! it belongs to — reverting "the last change" would revert whichever edit
//! happened to be newest, which is the bug this exists to prevent.

use std::collections::HashMap;
use ytm_core::{Playlist, PlaylistId, SetVideoId, Track};

/// Each variant stores what it needs to reverse itself.
#[derive(Debug, Clone)]
pub enum Mutation {
    CreatePlaylist {
        temp: Playlist,
    },
    RenamePlaylist {
        id: PlaylistId,
        previous: String,
        next: String,
    },
    DeletePlaylist {
        id: PlaylistId,
        index: usize,
        snapshot: Playlist,
    },
    AddTracks {
        playlist: PlaylistId,
        count: usize,
    },
    /// `(original index, track)` pairs, ascending, so re-insertion is exact.
    RemoveTracks {
        playlist: PlaylistId,
        removed: Vec<(usize, Track)>,
    },
}

#[derive(Default)]
pub struct MutationLog {
    next: u64,
    pending: HashMap<u64, Mutation>,
}

impl MutationLog {
    pub fn next_token(&mut self) -> u64 {
        self.next += 1;
        self.next
    }

    /// The token `next_token` will return, without consuming it. A create needs
    /// its token to build the temp id *before* the edit is applied; consuming
    /// one here would make the id and the mutation disagree.
    pub fn peek_token(&self) -> u64 {
        self.next + 1
    }

    pub fn insert(&mut self, token: u64, m: Mutation) {
        self.pending.insert(token, m);
    }

    /// Removing on take is what makes settling idempotent: a duplicated
    /// response cannot revert an edit twice.
    pub fn take(&mut self, token: u64) -> Option<Mutation> {
        self.pending.remove(&token)
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }
}

/// Entries the API still has to confirm; used for the removal SetVideoIds.
///
/// Tracks without one are skipped rather than faked. Playlist reads in
/// `ytmapi-rs` 0.3.3 do not carry `setVideoId` at all (PROGRESS.md open
/// question 1), so `None` is the common case, not an edge case.
pub fn set_video_ids(removed: &[(usize, Track)]) -> Vec<SetVideoId> {
    removed
        .iter()
        .filter_map(|(_, t)| t.set_video_id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::AppState;
    use ytm_core::{Playlist, PlaylistId, Track};

    #[test]
    fn peek_shows_the_next_token_without_taking_it() {
        let mut log = MutationLog::default();
        let peeked = log.peek_token();
        assert_eq!(log.next_token(), peeked, "peek must not consume");
    }

    #[test]
    fn tokens_are_unique() {
        let mut log = MutationLog::default();
        let a = log.next_token();
        let b = log.next_token();
        assert_ne!(a, b);
    }

    #[test]
    fn create_applies_immediately_then_commits() {
        let mut s = AppState::default();
        let token = s.begin_mutation(Mutation::CreatePlaylist {
            temp: Playlist::stub("temp-1", "New List"),
        });
        // FR-C6: the row is visible before the server confirms.
        assert_eq!(s.playlists.len(), 1);
        assert_eq!(s.playlists[0].title, "New List");

        s.commit(token, Some(PlaylistId::from("real-id")));
        assert_eq!(
            s.playlists[0].id,
            PlaylistId::from("real-id"),
            "temp id must be replaced"
        );
        assert!(s.pending.is_empty(), "commit clears the pending entry");
    }

    #[test]
    fn create_rolls_back_on_failure() {
        let mut s = AppState::default();
        let token = s.begin_mutation(Mutation::CreatePlaylist {
            temp: Playlist::stub("temp-1", "New List"),
        });
        s.rollback(token);
        assert!(s.playlists.is_empty(), "the optimistic row must disappear");
    }

    #[test]
    fn rename_restores_the_previous_title_on_failure() {
        let mut s = AppState {
            playlists: vec![Playlist::stub("p1", "Old Name")],
            ..Default::default()
        };
        let token = s.begin_mutation(Mutation::RenamePlaylist {
            id: PlaylistId::from("p1"),
            previous: "Old Name".into(),
            next: "New Name".into(),
        });
        assert_eq!(s.playlists[0].title, "New Name");
        s.rollback(token);
        assert_eq!(s.playlists[0].title, "Old Name");
    }

    #[test]
    fn delete_restores_the_playlist_at_its_original_index() {
        let mut s = AppState {
            playlists: vec![
                Playlist::stub("p1", "First"),
                Playlist::stub("p2", "Second"),
                Playlist::stub("p3", "Third"),
            ],
            ..Default::default()
        };
        let token = s.begin_mutation(Mutation::DeletePlaylist {
            id: PlaylistId::from("p2"),
            index: 1,
            snapshot: s.playlists[1].clone(),
        });
        assert_eq!(s.playlists.len(), 2);
        s.rollback(token);
        assert_eq!(s.playlists.len(), 3);
        assert_eq!(
            s.playlists[1].title, "Second",
            "must return to its original position"
        );
    }

    #[test]
    fn remove_tracks_restores_them_on_failure() {
        let mut s = AppState {
            tracks: vec![
                Track::stub("v1", "A"),
                Track::stub("v2", "B"),
                Track::stub("v3", "C"),
            ],
            ..Default::default()
        };
        let token = s.begin_mutation(Mutation::RemoveTracks {
            playlist: PlaylistId::from("p1"),
            removed: vec![(1, s.tracks[1].clone())],
        });
        assert_eq!(s.tracks.len(), 2);
        s.rollback(token);
        assert_eq!(s.tracks.len(), 3);
        assert_eq!(s.tracks[1].title, "B");
    }

    #[test]
    fn several_removed_tracks_all_return_to_their_own_indices() {
        // Re-inserting ascending only lands right if each index is interpreted
        // against the list as it is being rebuilt.
        let mut s = AppState {
            tracks: vec![
                Track::stub("v1", "A"),
                Track::stub("v2", "B"),
                Track::stub("v3", "C"),
                Track::stub("v4", "D"),
            ],
            ..Default::default()
        };
        let token = s.begin_mutation(Mutation::RemoveTracks {
            playlist: PlaylistId::from("p1"),
            removed: vec![(1, s.tracks[1].clone()), (3, s.tracks[3].clone())],
        });
        assert_eq!(s.tracks.len(), 2);
        s.rollback(token);
        let titles: Vec<_> = s.tracks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, ["A", "B", "C", "D"]);
    }

    #[test]
    fn a_late_failure_reverts_the_right_edit_when_two_are_in_flight() {
        // The user renamed two playlists quickly; only the second fails.
        let mut s = AppState {
            playlists: vec![Playlist::stub("p1", "One"), Playlist::stub("p2", "Two")],
            ..Default::default()
        };
        let t1 = s.begin_mutation(Mutation::RenamePlaylist {
            id: PlaylistId::from("p1"),
            previous: "One".into(),
            next: "Uno".into(),
        });
        let t2 = s.begin_mutation(Mutation::RenamePlaylist {
            id: PlaylistId::from("p2"),
            previous: "Two".into(),
            next: "Dos".into(),
        });
        s.rollback(t2);
        assert_eq!(
            s.playlists[0].title, "Uno",
            "the successful edit must survive"
        );
        assert_eq!(s.playlists[1].title, "Two", "only the failed edit reverts");
        s.commit(t1, None);
        assert!(s.pending.is_empty());
    }

    #[test]
    fn rollback_of_an_unknown_token_is_a_no_op() {
        let mut s = AppState::default();
        s.rollback(9999); // must not panic
        assert!(s.playlists.is_empty());
    }

    #[test]
    fn a_token_cannot_be_settled_twice() {
        // A duplicated MutationFailed must not revert an unrelated later edit.
        let mut s = AppState {
            playlists: vec![Playlist::stub("p1", "One")],
            ..Default::default()
        };
        let t = s.begin_mutation(Mutation::DeletePlaylist {
            id: PlaylistId::from("p1"),
            index: 0,
            snapshot: s.playlists[0].clone(),
        });
        s.rollback(t);
        s.rollback(t);
        assert_eq!(s.playlists.len(), 1, "the second rollback must do nothing");
    }

    #[test]
    fn the_mutation_events_settle_the_edit_they_name() {
        use crate::event::AppEvent;
        let mut s = AppState {
            playlists: vec![Playlist::stub("p1", "Old")],
            ..Default::default()
        };
        let token = s.begin_mutation(Mutation::RenamePlaylist {
            id: PlaylistId::from("p1"),
            previous: "Old".into(),
            next: "New".into(),
        });
        s.apply(AppEvent::MutationFailed {
            token,
            message: "rejected".into(),
        });
        assert_eq!(s.playlists[0].title, "Old", "the event must roll back");
        assert_eq!(s.toasts.len(), 1, "and still tell the user");
        assert!(s.pending.is_empty());
    }

    #[test]
    fn a_successful_mutation_event_clears_the_pending_entry() {
        use crate::event::AppEvent;
        let mut s = AppState::default();
        let token = s.begin_mutation(Mutation::CreatePlaylist {
            temp: Playlist::stub("temp-1", "New List"),
        });
        s.apply(AppEvent::MutationOk {
            token,
            real_id: Some(PlaylistId::from("real-id")),
            message: "created".into(),
        });
        assert!(s.pending.is_empty());
        assert_eq!(s.playlists.len(), 1, "the row stays");
        assert_eq!(
            s.playlists[0].id,
            PlaylistId::from("real-id"),
            "and picks up the server's id"
        );
    }

    #[test]
    fn set_video_ids_skips_entries_the_api_cannot_remove() {
        // Open question 1: playlist reads have no setVideoId, so this is
        // routinely None and the caller must not send a bogus id.
        let with = Track {
            set_video_id: Some(ytm_core::SetVideoId::from("s1")),
            ..Track::stub("v1", "A")
        };
        let without = Track::stub("v2", "B");
        let ids = set_video_ids(&[(0, with), (1, without)]);
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], ytm_core::SetVideoId::from("s1"));
    }
}
