//! What the music player is playing, and controlling it.
//!
//! [`Media::start`] connects to the active player on a background thread and keeps
//! a [`Track`] snapshot fresh; [`Media::send`] sends it a [`Command`].
//!
//! - **Linux:** MPRIS over D-Bus (Spotify, browsers, most desktop players).
//! - **Windows:** the system's media controls (the same players and more).
//! - **Other platforms:** not implemented yet. There is never a track, and the app
//!   hides the controls that would need one.
//!
//! Commands apply *optimistically*: the snapshot shows the new state right away
//! and keeps showing it until the player confirms it (or a couple of seconds pass),
//! so the controls don't flicker back while the player catches up.

use crate::audio::player_matches;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as backend;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows as backend;

/// A track's cover image.
#[derive(Clone, Debug)]
pub enum Artwork {
    /// Where it is (`file://` or `http(s)://`).
    Url(String),
    /// The image file itself (PNG, JPEG…), from players that hand it over.
    File(Arc<[u8]>),
}

impl PartialEq for Artwork {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Url(a), Self::Url(b)) => a == b,
            // Compared every frame, and nearly always the very same.
            (Self::File(a), Self::File(b)) => Arc::ptr_eq(a, b) || a == b,
            _ => false,
        }
    }
}

/// A snapshot of the current track and the player's state.
#[derive(Clone, Debug)]
pub struct Track {
    /// Track title (may be empty).
    pub title: String,
    /// Artist names joined with ", " (may be empty).
    pub artist: String,
    /// Album title (may be empty).
    pub album: String,
    /// Cover image, when the player provides one.
    pub art: Option<Artwork>,
    /// The song's own URL (`file://` for local files), used to find `.lrc` files.
    /// Players don't say on Windows.
    pub url: Option<String>,
    /// Track length, when the player knows it.
    pub length: Option<Duration>,
    /// Playback position at `sampled_at`; see [`Track::position_at`].
    pub position: Duration,
    /// When `position` was sampled, so the UI can interpolate between polls.
    pub sampled_at: Instant,
    /// Playing (as opposed to paused or stopped).
    pub playing: bool,
    /// Repeating the track or playlist.
    pub repeat: bool,
    /// Player volume, 0.0..=1.0, when the player exposes it. (On Windows, the
    /// player's volume in the system's volume mixer.)
    pub volume: Option<f64>,
    /// Whether the player accepts [`Command::SeekTo`].
    pub can_seek: bool,
    /// What the player calls itself to the system (empty when unknown). On Linux, its
    /// D-Bus name after `org.mpris.MediaPlayer2.`, e.g. `spotify`,
    /// `firefox.instance_1_23` or `org.kde.elisa`; on Windows, its executable's name,
    /// e.g. `spotify` or `firefox`.
    pub player_bus: String,
    /// The player's display name, e.g. "Spotify" or "Mozilla Firefox".
    pub player_name: String,
}

impl Track {
    /// Position extrapolated to `now` while playing.
    pub fn position_at(&self, now: Instant) -> Duration {
        let mut pos = self.position;
        if self.playing {
            pos += now.saturating_duration_since(self.sampled_at);
        }
        match self.length {
            Some(len) => pos.min(len),
            None => pos,
        }
    }
}

/// How long an optimistic change wins over what the player reports. Players take a
/// moment to apply commands, and a poll already in flight carries the old state.
const PENDING_FOR: Duration = Duration::from_secs(2);
/// A reported position this close to the expected one confirms a seek.
const SEEK_MATCH: Duration = Duration::from_millis(1500);

/// The target media to get audio from.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum MediaTarget {
    /// Follow whichever player is currently active.
    #[default]
    Any,
    /// Follow only the player matching this app key.
    App(String),
}

impl MediaTarget {
    fn matches(&self, bus: &str, identity: &str) -> bool {
        match self {
            Self::Any => true,
            Self::App(key) => player_matches(bus, identity, std::slice::from_ref(key)),
        }
    }
}

// A media backend request.
pub(crate) enum Request {
    // A select media target to change to
    Target(MediaTarget),
    // See Command
    Command(Command),
}

/// Changes we've asked for but the player hasn't confirmed yet, with when we asked.
#[derive(Default)]
struct Pending {
    playing: Option<(bool, Instant)>,
    /// Target position, and when the seek was sent.
    seek: Option<(Duration, Instant)>,
    repeat: Option<(bool, Instant)>,
    volume: Option<(f64, Instant)>,
}

/// State shared with the backend thread.
#[derive(Default)]
pub(crate) struct Shared {
    target: MediaTarget,
    track: Option<Track>,
    pending: Pending,
    /// Counts connections to the player: an older one may still be stuck in a call,
    /// and has nothing more to say.
    connection: u32,
}

impl Shared {
    /// Store a fresh poll, keeping our own unconfirmed changes on top of it so the
    /// controls don't flicker back to the old state while the player catches up.
    pub(crate) fn publish(&mut self, snapshot: Option<Track>, now: Instant) {
        let ts = snapshot.filter(|np| self.target.matches(&np.player_bus, &np.player_name));
        let Some(mut t) = ts else {
            self.track = None;
            self.pending = Pending::default();
            return;
        };
        // A different track makes any pending seek meaningless.
        let same_track = self
            .track
            .as_ref()
            .is_some_and(|old| old.title == t.title && old.url == t.url);
        if !same_track {
            self.pending = Pending::default();
        }
        let p = &mut self.pending;
        let live = |at: &Instant| now.saturating_duration_since(*at) < PENDING_FOR;

        if let Some((want, at)) = p.playing {
            if t.playing == want || !live(&at) {
                p.playing = None;
            } else {
                t.position = t.position_at(now);
                t.sampled_at = now;
                t.playing = want;
            }
        }
        if let Some((target, at)) = p.seek {
            let elapsed = if t.playing {
                now.saturating_duration_since(at)
            } else {
                Duration::ZERO
            };
            let expected = target + elapsed;
            let reported = t.position_at(now);
            let off = reported.max(expected) - reported.min(expected);
            if off < SEEK_MATCH || !live(&at) {
                p.seek = None;
            } else {
                t.position = target;
                t.sampled_at = at;
            }
        }
        if let Some((want, at)) = p.repeat {
            if t.repeat == want || !live(&at) {
                p.repeat = None;
            } else {
                t.repeat = want;
            }
        }
        if let Some((want, at)) = p.volume {
            let close = t.volume.is_some_and(|v| (v - want).abs() < 0.01);
            if close || !live(&at) {
                p.volume = None;
            } else {
                t.volume = Some(want);
            }
        }
        self.track = Some(t);
    }

    /// Apply a command locally right away and remember it until the player agrees.
    fn optimistic(&mut self, command: &Command, now: Instant) {
        let Some(t) = self.track.as_mut() else {
            return;
        };
        let p = &mut self.pending;
        match command {
            Command::PlayPause => {
                t.position = t.position_at(now);
                t.sampled_at = now;
                t.playing = !t.playing;
                p.playing = Some((t.playing, now));
                // Keep a pending seek's clock consistent with the new play state.
                if p.seek.is_some() {
                    p.seek = Some((t.position, now));
                }
            }
            Command::SeekTo(pos) => {
                t.position = *pos;
                t.sampled_at = now;
                p.seek = Some((*pos, now));
            }
            Command::SetRepeat(on) => {
                t.repeat = *on;
                p.repeat = Some((*on, now));
            }
            Command::SetVolume(v) => {
                t.volume = Some(*v);
                p.volume = Some((*v, now));
            }
            Command::Next | Command::Previous => {}
        }
    }
}

/// Something to ask the player to do.
#[derive(Clone, Debug)]
pub enum Command {
    /// Toggle between playing and paused.
    PlayPause,
    /// Skip to the next track.
    Next,
    /// Go back to the previous track.
    Previous,
    /// Jump to a position in the current track.
    SeekTo(Duration),
    /// Turn repeat on or off.
    SetRepeat(bool),
    /// Set the player's volume, 0.0..=1.0.
    SetVolume(f64),
}

/// Follow the player from a thread of its own.
fn connect(
    state: Arc<Mutex<Shared>>,
    target: MediaTarget,
    connection: u32,
) -> Option<Sender<Request>> {
    // Tests drive the real app; never let them control what's playing here.
    if cfg!(test) {
        return None;
    }
    backend::spawn(state, target, connection)
}

/// Handle to the media backend. Cheap to query every frame.
pub struct Media {
    state: Arc<Mutex<Shared>>,
    requests: Option<Sender<Request>>,
}

impl Media {
    /// Start following the active player (in the background; this returns at once).
    pub fn start() -> Self {
        let state = Arc::new(Mutex::new(Shared::default()));
        let requests = connect(state.clone(), MediaTarget::Any, 0);
        Self { state, requests }
    }

    /// Connect to the player again and read everything anew, in case the connection is
    /// stuck (or never worked).
    pub fn refresh(&mut self) {
        let Ok(mut shared) = self.state.lock() else {
            return;
        };
        shared.connection += 1;
        // What's shown stays until there's something new, but nothing that was asked
        // for hides it.
        shared.pending = Pending::default();
        let (target, connection) = (shared.target.clone(), shared.connection);
        drop(shared);
        // (The old thread ends with its requests.)
        self.requests = connect(self.state.clone(), target, connection);
    }

    /// Sets the media target of the shared state.
    pub fn set_target(&self, target: MediaTarget) {
        if let Ok(mut shared) = self.state.lock() {
            if shared.target == target {
                return;
            }
            shared.target = target.clone();
            shared.pending = Pending::default();
            if shared
                .track
                .as_ref()
                .is_some_and(|np| !shared.target.matches(&np.player_bus, &np.player_name))
            {
                shared.track = None;
            }
        }
        if let Some(tx) = &self.requests {
            let _ = tx.send(Request::Target(target));
        }
    }

    /// The latest snapshot, or `None` when no player is running (or on platforms
    /// without a media backend).
    pub fn get_track(&self) -> Option<Track> {
        self.state.lock().ok().and_then(|s| s.track.clone())
    }

    /// Send `command` to the player. Does nothing without one.
    pub fn send(&self, command: Command) {
        // Update local state optimistically so the UI responds immediately.
        if let Ok(mut shared) = self.state.lock() {
            shared.optimistic(&command, Instant::now());
        }
        if let Some(tx) = &self.requests {
            let _ = tx.send(Request::Command(command));
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod backend {
    use super::{MediaTarget, Request, Shared};
    use std::sync::mpsc::Sender;
    use std::sync::{Arc, Mutex};

    /// No media backend here yet, so there's never a track; `Platform::media` hides
    /// the controls.
    pub fn spawn(
        _state: Arc<Mutex<Shared>>,
        _target: MediaTarget,
        _connection: u32,
    ) -> Option<Sender<Request>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refreshing_connects_again() {
        let mut media = Media::start();
        media.set_target(MediaTarget::App("spotify".into()));
        media.refresh();
        media.refresh();
        let shared = media.state.lock().unwrap();
        assert_eq!(shared.connection, 2);
        assert_eq!(shared.target, MediaTarget::App("spotify".into()));
    }

    fn track(playing: bool, pos_s: u64, at: Instant) -> Track {
        Track {
            title: "Song".into(),
            artist: String::new(),
            album: String::new(),
            art: None,
            url: None,
            length: Some(Duration::from_secs(200)),
            position: Duration::from_secs(pos_s),
            sampled_at: at,
            playing,
            repeat: false,
            volume: Some(0.5),
            can_seek: true,
            player_bus: "test".into(),
            player_name: "Test".into(),
        }
    }

    #[test]
    fn stale_polls_dont_undo_play_pause() {
        let t0 = Instant::now();
        let mut s = Shared::default();
        s.publish(Some(track(true, 10, t0)), t0);
        s.optimistic(&Command::PlayPause, t0);
        assert!(!s.track.as_ref().unwrap().playing);
        // The player still reports "playing" for a moment: we keep showing paused.
        let t1 = t0 + Duration::from_millis(500);
        s.publish(Some(track(true, 10, t1)), t1);
        assert!(!s.track.as_ref().unwrap().playing);
        // Once it agrees, the pending change is done...
        let t2 = t0 + Duration::from_millis(900);
        s.publish(Some(track(false, 10, t2)), t2);
        assert!(s.pending.playing.is_none());
        // ...and a later real change from the player shows up right away.
        let t3 = t0 + Duration::from_millis(1200);
        s.publish(Some(track(true, 10, t3)), t3);
        assert!(s.track.as_ref().unwrap().playing);
    }

    #[test]
    fn stale_polls_dont_undo_a_seek() {
        let t0 = Instant::now();
        let mut s = Shared::default();
        s.publish(Some(track(true, 10, t0)), t0);
        s.optimistic(&Command::SeekTo(Duration::from_secs(120)), t0);
        // A poll that started before the seek still says ~10 s.
        let t1 = t0 + Duration::from_millis(400);
        s.publish(Some(track(true, 10, t1)), t1);
        let pos = s.track.as_ref().unwrap().position_at(t1);
        assert!(pos >= Duration::from_secs(120), "{pos:?}");
        // The player reaches the new spot: confirmed.
        let t2 = t0 + Duration::from_millis(800);
        s.publish(Some(track(true, 120, t2)), t2);
        assert!(s.pending.seek.is_none());
    }

    #[test]
    fn unconfirmed_changes_give_up_after_a_while() {
        let t0 = Instant::now();
        let mut s = Shared::default();
        s.publish(Some(track(true, 10, t0)), t0);
        s.optimistic(&Command::PlayPause, t0);
        // The player ignored the command: show what it really does.
        let late = t0 + PENDING_FOR + Duration::from_millis(100);
        s.publish(Some(track(true, 12, late)), late);
        assert!(s.track.as_ref().unwrap().playing);
    }
}
