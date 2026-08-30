//! Player impl for tests. Records commands, lets tests push events in.

use crate::player::*;
use std::sync::Mutex;
use tokio::sync::mpsc;

pub struct MockPlayer {
    sent: Mutex<Vec<PlayerCommand>>,
    events: mpsc::UnboundedSender<PlayerEvent>,
}

impl MockPlayer {
    pub fn new() -> (Self, mpsc::UnboundedReceiver<PlayerEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Self {
                sent: Mutex::new(Vec::new()),
                events: tx,
            },
            rx,
        )
    }

    pub fn commands(&self) -> Vec<PlayerCommand> {
        self.sent.lock().unwrap().clone()
    }

    /// Simulate the player reporting something.
    pub fn emit(&self, e: PlayerEvent) {
        let _ = self.events.send(e);
    }
}

impl Player for MockPlayer {
    fn send(&self, cmd: PlayerCommand) -> Result<(), PlayerError> {
        self.sent.lock().unwrap().push(cmd);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ytm_core::Track;

    #[test]
    fn records_commands_in_order() {
        let (p, _rx) = MockPlayer::new();
        p.send(PlayerCommand::TogglePause).unwrap();
        p.send(PlayerCommand::SetVolume(30)).unwrap();
        assert_eq!(p.commands().len(), 2);
        assert!(matches!(p.commands()[1], PlayerCommand::SetVolume(30)));
    }

    #[tokio::test]
    async fn emit_delivers_an_event_to_the_receiver() {
        let (p, mut rx) = MockPlayer::new();
        p.emit(PlayerEvent::StateChanged(PlaybackState::Playing));
        assert!(matches!(
            rx.recv().await,
            Some(PlayerEvent::StateChanged(PlaybackState::Playing))
        ));
    }

    #[test]
    fn play_now_records_the_track() {
        let (p, _rx) = MockPlayer::new();
        p.send(PlayerCommand::PlayNow(Track::stub("v9", "Song")))
            .unwrap();
        match &p.commands()[0] {
            PlayerCommand::PlayNow(t) => assert_eq!(t.video_id.as_str(), "v9"),
            other => panic!("expected PlayNow, got {other:?}"),
        }
    }
}
