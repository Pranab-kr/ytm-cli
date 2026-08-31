//! Queue ordering, shuffle, and repeat. Pure — no I/O, fully unit-tested.

use crate::player::RepeatMode;
use rand::seq::SliceRandom;
use ytm_core::Track;

#[derive(Default)]
pub struct Queue {
    items: Vec<Track>,
    current: Option<usize>,
    repeat: RepeatMode,
    shuffle: bool,
    /// Original order, kept so disabling shuffle can restore it.
    unshuffled: Option<Vec<Track>>,
}

impl Queue {
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn tracks(&self) -> &[Track] {
        &self.items
    }
    pub fn current_index(&self) -> Option<usize> {
        self.current
    }
    pub fn current(&self) -> Option<&Track> {
        self.items.get(self.current?)
    }
    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }
    pub fn shuffled(&self) -> bool {
        self.shuffle
    }

    pub fn set_repeat(&mut self, m: RepeatMode) {
        self.repeat = m;
    }

    pub fn push_back(&mut self, mut new: Vec<Track>) {
        if new.is_empty() {
            return;
        }
        // The saved order is the restore point for `set_shuffle(false)`, so it
        // has to grow too — otherwise unshuffling silently drops whatever was
        // enqueued while shuffle was on.
        if let Some(v) = self.unshuffled.as_mut() {
            v.extend(new.iter().cloned());
        }
        self.items.append(&mut new);
        if self.current.is_none() {
            self.current = Some(0);
        }
    }

    /// Insert so these play immediately after the current track.
    pub fn push_next(&mut self, new: Vec<Track>) {
        if new.is_empty() {
            return;
        }
        match self.current {
            Some(i) => {
                // Mirror into the saved order, next to the same track, so the
                // insert survives unshuffling.
                if let Some(v) = self.unshuffled.as_mut() {
                    let at = self
                        .items
                        .get(i)
                        .and_then(|cur| v.iter().position(|t| t.video_id == cur.video_id))
                        .map(|p| p + 1)
                        .unwrap_or(v.len());
                    v.splice(at..at, new.iter().cloned());
                }
                let at = (i + 1).min(self.items.len());
                self.items.splice(at..at, new);
            }
            None => {
                if let Some(v) = self.unshuffled.as_mut() {
                    v.extend(new.iter().cloned());
                }
                self.items = new;
                self.current = Some(0);
            }
        }
    }

    /// Next track per the repeat mode. `None` means playback should stop.
    pub fn advance(&mut self) -> Option<&Track> {
        let i = self.current?;
        let next = match self.repeat {
            RepeatMode::One => i,
            RepeatMode::Off => {
                if i + 1 >= self.items.len() {
                    return None;
                }
                i + 1
            }
            RepeatMode::All => {
                if self.items.is_empty() {
                    return None;
                }
                (i + 1) % self.items.len()
            }
        };
        self.current = Some(next);
        self.items.get(next)
    }

    /// Previous track, clamped at the start.
    pub fn previous(&mut self) -> Option<&Track> {
        let i = self.current?;
        let prev = i.saturating_sub(1);
        self.current = Some(prev);
        self.items.get(prev)
    }

    /// Remove by index, keeping the same track playing where possible.
    /// Make `idx` the current entry, if it exists.
    ///
    /// Returns the track so the actor can play it without a second lookup that
    /// could disagree about which row is current.
    pub fn jump_to(&mut self, idx: usize) -> Option<Track> {
        let t = self.items.get(idx)?.clone();
        self.current = Some(idx);
        Some(t)
    }

    pub fn remove(&mut self, idx: usize) {
        if idx >= self.items.len() {
            return;
        }
        let gone = self.items.remove(idx);
        if let Some(v) = self.unshuffled.as_mut() {
            // Keep the saved order consistent with the live list.
            if let Some(p) = v.iter().position(|t| t.video_id == gone.video_id) {
                v.remove(p);
            }
        }
        self.current = match self.current {
            None => None,
            Some(_) if self.items.is_empty() => None,
            Some(c) if idx < c => Some(c - 1),
            // Removing the current entry: the next track slides into this slot.
            Some(c) if idx == c => Some(c.min(self.items.len() - 1)),
            Some(c) => Some(c),
        };
    }

    pub fn move_item(&mut self, from: usize, to: usize) {
        if from >= self.items.len() || to >= self.items.len() || from == to {
            return;
        }
        let t = self.items.remove(from);
        self.items.insert(to, t);
        if let Some(c) = self.current {
            self.current = Some(if c == from {
                to
            } else if from < c && to >= c {
                c - 1
            } else if from > c && to <= c {
                c + 1
            } else {
                c
            });
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.current = None;
        self.unshuffled = None;
    }

    /// Shuffle the tail while keeping the current track in place.
    pub fn set_shuffle(&mut self, on: bool) {
        if on == self.shuffle {
            return;
        }
        self.shuffle = on;
        if on {
            self.unshuffled = Some(self.items.clone());
            // Split by index, not by id: a playlist may hold the same video
            // twice, and filtering on `video_id` would delete every copy of the
            // current track rather than lifting the one that is playing.
            let mut rest = std::mem::take(&mut self.items);
            let keep = self
                .current
                .filter(|i| *i < rest.len())
                .map(|i| rest.remove(i));
            rest.shuffle(&mut rand::rng());
            self.items = match keep {
                Some(k) => {
                    let mut v = vec![k];
                    v.extend(rest);
                    self.current = Some(0);
                    v
                }
                None => rest,
            };
        } else if let Some(orig) = self.unshuffled.take() {
            let keep = self
                .current
                .and_then(|i| self.items.get(i).map(|t| t.video_id.clone()));
            self.items = orig;
            self.current = keep.and_then(|id| self.items.iter().position(|t| t.video_id == id));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ytm_core::Track;

    fn tracks(n: usize) -> Vec<Track> {
        (0..n)
            .map(|i| Track::stub(&format!("v{i}"), &format!("Track {i}")))
            .collect()
    }

    #[test]
    fn empty_queue_has_no_current_track() {
        let q = Queue::default();
        assert!(q.current().is_none());
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn push_back_appends_and_first_push_becomes_current() {
        let mut q = Queue::default();
        q.push_back(tracks(3));
        assert_eq!(q.current().unwrap().video_id.as_str(), "v0");
        assert_eq!(q.len(), 3);
    }

    #[test]
    fn advance_moves_to_the_next_track() {
        let mut q = Queue::default();
        q.push_back(tracks(3));
        assert_eq!(q.advance().unwrap().video_id.as_str(), "v1");
        assert_eq!(q.advance().unwrap().video_id.as_str(), "v2");
    }

    #[test]
    fn advance_past_the_end_returns_none_when_repeat_is_off() {
        let mut q = Queue::default();
        q.push_back(tracks(2));
        q.advance();
        assert!(q.advance().is_none(), "queue should run out");
    }

    #[test]
    fn repeat_all_wraps_to_the_start() {
        let mut q = Queue::default();
        q.push_back(tracks(2));
        q.set_repeat(RepeatMode::All);
        q.advance();
        assert_eq!(q.advance().unwrap().video_id.as_str(), "v0");
    }

    #[test]
    fn repeat_one_replays_the_same_track() {
        let mut q = Queue::default();
        q.push_back(tracks(3));
        q.set_repeat(RepeatMode::One);
        assert_eq!(q.advance().unwrap().video_id.as_str(), "v0");
        assert_eq!(q.advance().unwrap().video_id.as_str(), "v0");
    }

    #[test]
    fn previous_moves_back_and_stops_at_the_first_track() {
        let mut q = Queue::default();
        q.push_back(tracks(3));
        q.advance();
        assert_eq!(q.previous().unwrap().video_id.as_str(), "v0");
        assert_eq!(
            q.previous().unwrap().video_id.as_str(),
            "v0",
            "clamps at the start"
        );
    }

    #[test]
    fn push_next_inserts_directly_after_current() {
        let mut q = Queue::default();
        q.push_back(tracks(3)); // v0 v1 v2, current v0
        q.push_next(vec![Track::stub("x", "Jumped")]); // v0 x v1 v2
        assert_eq!(q.advance().unwrap().video_id.as_str(), "x");
    }

    #[test]
    fn remove_before_current_keeps_the_same_track_playing() {
        let mut q = Queue::default();
        q.push_back(tracks(3));
        q.advance(); // current v1 at index 1
        q.remove(0); // removing v0 shifts indices
        assert_eq!(
            q.current().unwrap().video_id.as_str(),
            "v1",
            "must not skip"
        );
        assert_eq!(q.current_index(), Some(0));
    }

    #[test]
    fn removing_the_current_track_moves_to_the_next() {
        let mut q = Queue::default();
        q.push_back(tracks(3));
        q.remove(0);
        assert_eq!(q.current().unwrap().video_id.as_str(), "v1");
    }

    #[test]
    fn removing_the_last_remaining_track_empties_the_queue() {
        let mut q = Queue::default();
        q.push_back(tracks(1));
        q.remove(0);
        assert!(q.current().is_none());
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn move_item_reorders_and_tracks_the_current_index() {
        let mut q = Queue::default();
        q.push_back(tracks(3)); // v0 v1 v2, current v0
        q.move_item(0, 2); // v1 v2 v0, current still v0
        assert_eq!(q.tracks()[2].video_id.as_str(), "v0");
        assert_eq!(q.current().unwrap().video_id.as_str(), "v0");
        assert_eq!(q.current_index(), Some(2));
    }

    #[test]
    fn shuffle_preserves_the_current_track_and_the_full_set() {
        let mut q = Queue::default();
        q.push_back(tracks(6));
        q.advance();
        let before = q.current().unwrap().video_id.clone();
        q.set_shuffle(true);
        assert_eq!(
            q.current().unwrap().video_id,
            before,
            "shuffle must not change what is playing"
        );
        assert_eq!(q.len(), 6, "shuffle must not lose tracks");
    }

    #[test]
    fn disabling_shuffle_restores_the_original_order() {
        let mut q = Queue::default();
        q.push_back(tracks(5));
        q.set_shuffle(true);
        q.set_shuffle(false);
        let ids: Vec<_> = q.tracks().iter().map(|t| t.video_id.0.clone()).collect();
        assert_eq!(ids, vec!["v0", "v1", "v2", "v3", "v4"]);
    }

    #[test]
    fn clear_empties_everything() {
        let mut q = Queue::default();
        q.push_back(tracks(3));
        q.clear();
        assert_eq!(q.len(), 0);
        assert!(q.current().is_none());
        assert_eq!(q.current_index(), None);
    }

    #[test]
    fn a_track_enqueued_while_shuffled_survives_unshuffling() {
        // `unshuffled` is the restore point, so an append that skips it is
        // silently dropped the moment the user presses `s` again.
        let mut q = Queue::default();
        q.push_back(tracks(3));
        q.set_shuffle(true);
        q.push_back(vec![Track::stub("late", "Added while shuffled")]);
        assert_eq!(q.len(), 4);
        q.set_shuffle(false);
        assert_eq!(q.len(), 4, "unshuffling lost the enqueued track");
        assert!(
            q.tracks().iter().any(|t| t.video_id.as_str() == "late"),
            "the enqueued track must still be there"
        );
    }

    #[test]
    fn a_play_next_insert_while_shuffled_survives_unshuffling() {
        let mut q = Queue::default();
        q.push_back(tracks(3));
        q.set_shuffle(true);
        q.push_next(vec![Track::stub("soon", "Play next")]);
        q.set_shuffle(false);
        assert!(
            q.tracks().iter().any(|t| t.video_id.as_str() == "soon"),
            "unshuffling lost the play-next track"
        );
    }

    #[test]
    fn shuffling_keeps_every_copy_of_a_repeated_track() {
        // A playlist may legitimately hold the same video twice. Excluding the
        // current track by id rather than by index deletes all of its copies.
        let mut q = Queue::default();
        q.push_back(vec![
            Track::stub("dup", "Same"),
            Track::stub("v1", "Other"),
            Track::stub("dup", "Same"),
        ]);
        q.set_shuffle(true);
        assert_eq!(q.len(), 3, "shuffle dropped a repeated entry");
        assert_eq!(
            q.tracks()
                .iter()
                .filter(|t| t.video_id.as_str() == "dup")
                .count(),
            2,
            "both copies must survive"
        );
    }

    #[test]
    fn shuffling_an_empty_queue_is_harmless() {
        let mut q = Queue::default();
        q.set_shuffle(true);
        assert_eq!(q.len(), 0);
        q.set_shuffle(false);
        assert_eq!(q.len(), 0);
    }
}
