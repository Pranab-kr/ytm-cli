//! Album art via ratatui-image. Optional by nature: many terminals cannot
//! display images, and the UI must be complete without it (FR-U5).
//!
//! Nothing here is allowed to be load-bearing. A terminal with no image support,
//! a track with no thumbnail, a dead URL, and a pane too narrow to be legible
//! all take the same path: draw nothing, say nothing, and leave the layout
//! exactly as it would have been.

use ratatui::{Frame, layout::Rect};
use ratatui_image::{StatefulImage, picker::Picker, protocol::StatefulProtocol};
use std::collections::{HashMap, HashSet};

/// Below this the image is noise, not art.
const MIN_W: u16 = 10;
const MIN_H: u16 = 5;

pub fn should_draw(area: Rect) -> bool {
    area.width >= MIN_W && area.height >= MIN_H
}

pub struct ArtCache {
    /// `None` when the terminal cannot display images. Also the picker that
    /// builds protocol objects, so "enabled" and "can decode" cannot disagree.
    picker: Option<Picker>,
    /// Decoded protocol objects, keyed by URL.
    images: HashMap<String, StatefulProtocol>,
    in_flight: HashSet<String>,
    failed: HashSet<String>,
}

impl ArtCache {
    /// Probes the terminal. Returns a disabled cache when images are unsupported.
    ///
    /// **Call this after entering the alternate screen but before reading
    /// terminal events** — it writes a query sequence and reads the reply, so an
    /// event stream already draining stdin would eat the answer. (The plan says
    /// "before entering the alternate screen"; `Picker::from_query_stdio`'s own
    /// doc comment in ratatui-image 11.0.6 says after, and it is the one that
    /// has to be right.)
    ///
    /// It also blocks for up to 2s waiting on a terminal that never answers, so
    /// it must run *after* the first frame is drawn or it spends the whole NFR-1
    /// budget on a probe.
    pub fn detect() -> Self {
        match Picker::from_query_stdio() {
            Ok(picker) => {
                tracing::info!(protocol = ?picker.protocol_type(), "album art enabled");
                Self::with_picker(Some(picker))
            }
            Err(e) => {
                // Not an error the user needs to see: FR-U5 makes art optional.
                tracing::info!(reason = %e, "album art unavailable, continuing without it");
                Self::with_picker(None)
            }
        }
    }

    pub fn disabled() -> Self {
        Self::with_picker(None)
    }

    fn with_picker(picker: Option<Picker>) -> Self {
        Self {
            picker,
            images: HashMap::new(),
            in_flight: HashSet::new(),
            failed: HashSet::new(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.picker.is_some()
    }

    pub fn get(&self, url: &str) -> Option<&StatefulProtocol> {
        self.images.get(url)
    }

    /// True exactly once per URL, unless it later fails.
    pub fn should_fetch(&mut self, url: &str) -> bool {
        if self.images.contains_key(url)
            || self.in_flight.contains(url)
            || self.failed.contains(url)
        {
            return false;
        }
        self.in_flight.insert(url.to_owned());
        true
    }

    pub fn mark_failed(&mut self, url: &str) {
        self.in_flight.remove(url);
        self.failed.insert(url.to_owned());
    }

    /// Build the protocol object for a decoded image. A disabled cache drops it:
    /// there is nothing that could render it, and keeping it would leak memory
    /// for every track played.
    pub fn insert(&mut self, url: &str, image: image::DynamicImage) {
        self.in_flight.remove(url);
        let Some(picker) = self.picker.as_ref() else {
            return;
        };
        self.images
            .insert(url.to_owned(), picker.new_resize_protocol(image));
    }
}

/// Draw the art for whatever is playing, or nothing at all.
///
/// Takes `&mut ArtCache` because the protocol objects re-encode themselves when
/// the area changes — that is what makes the image survive a resize.
pub fn draw(f: &mut Frame, area: Rect, url: Option<&str>, art: &mut ArtCache) {
    if !should_draw(area) || !art.is_enabled() {
        return;
    }
    let Some(url) = url else { return };
    let Some(proto) = art.images.get_mut(url) else {
        return;
    };
    f.render_stateful_widget(StatefulImage::default(), area, proto);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn art_is_skipped_when_the_terminal_cannot_display_images() {
        // FR-U5: absence must be graceful, never an error or a broken box.
        let cache = ArtCache::disabled();
        assert!(!cache.is_enabled());
        assert!(cache.get("https://example.com/a.jpg").is_none());
    }

    #[test]
    fn a_url_is_requested_only_once() {
        let mut cache = ArtCache::disabled();
        assert!(cache.should_fetch("u1"), "first sighting fetches");
        assert!(!cache.should_fetch("u1"), "already in flight or cached");
    }

    #[test]
    fn a_failed_fetch_is_not_retried_forever() {
        let mut cache = ArtCache::disabled();
        cache.should_fetch("u1");
        cache.mark_failed("u1");
        assert!(
            !cache.should_fetch("u1"),
            "a dead URL must not be retried on every tick"
        );
    }

    #[test]
    fn a_zero_sized_area_is_skipped_without_panicking() {
        use ratatui::layout::Rect;
        assert!(!should_draw(Rect::new(0, 0, 0, 0)));
        assert!(
            !should_draw(Rect::new(0, 0, 4, 2)),
            "too small to be legible"
        );
        assert!(should_draw(Rect::new(0, 0, 20, 10)));
    }
}
