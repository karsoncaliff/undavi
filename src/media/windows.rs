//! Media players through the system's media controls (Windows): whatever shows up
//! next to the volume in the quick settings.
//!
//! They know the track, the position and the cover, and pass on commands. Not the
//! volume, though: that's the one of the player's app in the volume mixer (see
//! [`AppVolume`]). And not the song's file, so there are no `.lrc` files to find.

use super::{Artwork, Command, MediaTarget, Request, Shared, Track};
use crate::audio::windows::{AppVolume, Com, player_names};
use std::collections::HashMap;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as Properties,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Media::MediaPlaybackAutoRepeatMode as RepeatMode;
use windows::Storage::Streams::DataReader;
use windows::core::Result;

const POLL: Duration = Duration::from_millis(500);
/// How often the player's app is looked up in the volume mixer. (In between, its
/// volume is read from where it was.)
const MIXER_RESCAN: Duration = Duration::from_secs(2);
/// For this long after a new song starts, its cover is read again: players set it a
/// moment after the title.
const COVER_SETTLES: Duration = Duration::from_secs(4);
/// The biggest cover read.
const MAX_COVER: u64 = 10 * 1024 * 1024;
/// A position reported longer ago than this isn't extrapolated from.
const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

pub fn spawn(
    state: Arc<Mutex<Shared>>,
    mut target: MediaTarget,
    connection: u32,
) -> Option<Sender<Request>> {
    let (tx, rx) = mpsc::channel::<Request>();
    std::thread::Builder::new()
        .name("undavi-media".into())
        .spawn(move || {
            let _com = Com::init();
            let manager = match Manager::RequestAsync().and_then(|request| request.join()) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("undavi: media controls unavailable: {e}");
                    return;
                }
            };
            let mut reader = Reader::default();
            loop {
                let player = reader.resolve(&manager, &target);
                let snapshot = player.as_ref().and_then(|p| reader.read(p).ok());
                if let Ok(mut s) = state.lock() {
                    if s.connection != connection {
                        return;
                    }
                    s.publish(snapshot, Instant::now());
                }
                match rx.recv_timeout(POLL) {
                    Ok(Request::Target(new_target)) => {
                        target = new_target;
                        while let Ok(msg) = rx.try_recv() {
                            if let Request::Target(t) = msg {
                                target = t;
                            }
                        }
                    }
                    Ok(Request::Command(cmd)) => {
                        let mut active = player.or_else(|| reader.resolve(&manager, &target));
                        if let Some(p) = &active {
                            reader.apply(p, cmd);
                        }
                        while let Ok(msg) = rx.try_recv() {
                            match msg {
                                Request::Target(new_target) => {
                                    target = new_target;
                                    active = reader.resolve(&manager, &target);
                                }
                                Request::Command(cmd) => {
                                    if let Some(p) = &active {
                                        reader.apply(p, cmd);
                                    }
                                }
                            }
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        })
        .ok()?;
    Some(tx)
}

/// A player and how it's called.
struct Player {
    session: Session,
    id: String,
    bus: String,
    name: String,
}

/// What's remembered from one look at the player to the next.
#[derive(Default)]
struct Reader {
    /// Players' names, by app ID.
    names: HashMap<String, (String, String)>,
    /// The app ID of the player the rest is about.
    player: String,
    clock: Option<Clock>,
    cover: Cover,
    mixer: Option<(AppVolume, Instant)>,
}

impl Reader {
    /// The player to follow. Preferably one that's playing, and among those the one
    /// Windows considers current.
    fn resolve(&mut self, manager: &Manager, target: &MediaTarget) -> Option<Player> {
        let current = manager.GetCurrentSession().ok();
        let sessions = manager.GetSessions().ok();
        let all = current
            .into_iter()
            .chain(sessions.into_iter().flat_map(|s| s.into_iter()));
        let mut best: Option<(u8, Player)> = None;
        for session in all {
            let Some(player) = self.player(session) else {
                continue;
            };
            if !target.matches(&player.bus, &player.name) {
                continue;
            }
            let status = player
                .session
                .GetPlaybackInfo()
                .and_then(|i| i.PlaybackStatus());
            let rank = match status {
                Ok(Status::Playing) => return Some(player),
                Ok(Status::Paused) => 1,
                _ => 2,
            };
            if best.as_ref().is_none_or(|(best_rank, _)| rank < *best_rank) {
                best = Some((rank, player));
            }
        }
        best.map(|(_, p)| p)
    }

    fn player(&mut self, session: Session) -> Option<Player> {
        let id = session.SourceAppUserModelId().ok()?.to_string();
        let (bus, name) = self
            .names
            .entry(id.clone())
            .or_insert_with(|| player_names(&id))
            .clone();
        Some(Player {
            session,
            id,
            bus,
            name,
        })
    }

    fn read(&mut self, p: &Player) -> Result<Track> {
        let now = Instant::now();
        if self.player != p.id {
            self.player = p.id.clone();
            self.clock = None;
            self.mixer = None;
        }
        let properties = p.session.TryGetMediaPropertiesAsync()?.join()?;
        let info = p.session.GetPlaybackInfo()?;
        let playing = info.PlaybackStatus()? == Status::Playing;
        let repeat = info.AutoRepeatMode().and_then(|mode| mode.Value());
        let can_seek = info
            .Controls()
            .and_then(|c| c.IsPlaybackPositionEnabled())
            .unwrap_or(false);

        let timeline = p.session.GetTimelineProperties()?;
        let start = duration(timeline.StartTime()?.Duration);
        let length = duration(timeline.EndTime()?.Duration).saturating_sub(start);
        let updated = timeline.LastUpdatedTime()?.UniversalTime;
        let report = Report {
            position: duration(timeline.Position()?.Duration).saturating_sub(start),
            updated,
            age: age(updated),
            playing,
        };
        let clock = match &mut self.clock {
            Some(clock) => {
                clock.update(&report, now);
                clock
            }
            None => self.clock.insert(Clock::new(&report, now)),
        };
        let (position, sampled_at) = (clock.position, clock.sampled_at);

        let title = properties.Title()?.to_string();
        let mut artist = properties.Artist()?.to_string();
        if artist.is_empty() {
            artist = properties.AlbumArtist()?.to_string();
        }
        let album = properties.AlbumTitle()?.to_string();
        let song = [&p.id, &title, &artist, &album].map(Clone::clone);
        Ok(Track {
            art: self.cover.of(song, &properties, now),
            title,
            artist,
            album,
            url: None,
            length: (!length.is_zero()).then_some(length),
            position,
            sampled_at,
            playing,
            repeat: matches!(repeat, Ok(RepeatMode::Track | RepeatMode::List)),
            volume: self.mixer(p, now).get(),
            can_seek,
            player_bus: p.bus.clone(),
            player_name: p.name.clone(),
        })
    }

    /// The player's app in the volume mixer.
    fn mixer(&mut self, p: &Player, now: Instant) -> &AppVolume {
        let fresh = |(_, at): &(AppVolume, Instant)| now.duration_since(*at) < MIXER_RESCAN;
        if !self.mixer.as_ref().is_some_and(fresh) {
            self.mixer = None;
        }
        &self
            .mixer
            .get_or_insert_with(|| (AppVolume::of_player(&p.bus, &p.name), now))
            .0
    }

    fn apply(&mut self, p: &Player, cmd: Command) {
        let s = &p.session;
        let result = match cmd {
            Command::PlayPause => s.TryTogglePlayPauseAsync().and_then(|r| r.join()),
            Command::Next => s.TrySkipNextAsync().and_then(|r| r.join()),
            Command::Previous => s.TrySkipPreviousAsync().and_then(|r| r.join()),
            Command::SeekTo(pos) => {
                let start = s.GetTimelineProperties().and_then(|t| t.StartTime());
                let ticks = start.map_or(0, |t| t.Duration) + (pos.as_nanos() / 100) as i64;
                s.TryChangePlaybackPositionAsync(ticks)
                    .and_then(|r| r.join())
            }
            Command::SetRepeat(on) => {
                let mode = if on {
                    RepeatMode::List
                } else {
                    RepeatMode::None
                };
                s.TryChangeAutoRepeatModeAsync(mode).and_then(|r| r.join())
            }
            Command::SetVolume(v) => self.mixer(p, Instant::now()).set(v).map(|()| true),
        };
        match result {
            Ok(true) => {}
            Ok(false) => eprintln!("undavi: media command failed: the player refused it"),
            Err(e) => eprintln!("undavi: media command failed: {e}"),
        }
    }
}

/// A length of time in the system's units (100 ns). Negative ones count as none.
fn duration(ticks: i64) -> Duration {
    Duration::from_nanos(u64::try_from(ticks).unwrap_or(0).saturating_mul(100))
}

/// How long ago a moment in the system's time (100 ns since 1601) was. `None` for
/// the future, and for players that never set it.
fn age(ticks: i64) -> Option<Duration> {
    const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;
    let since_unix = duration(ticks.checked_sub(UNIX_EPOCH_TICKS)?);
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .checked_sub(since_unix)
        .filter(|age| *age < MAX_AGE)
}

/// What a player says about where it is.
struct Report {
    position: Duration,
    /// When it said so (in the system's time)...
    updated: i64,
    /// ...and how long ago that was.
    age: Option<Duration>,
    playing: bool,
}

/// Keeps track of where the song is: players only report it every few seconds, or
/// just when it jumps.
struct Clock {
    reported: Duration,
    updated: i64,
    playing: bool,
    /// When this was last updated.
    looked: Instant,
    /// Where the song was at `sampled_at`.
    position: Duration,
    sampled_at: Instant,
}

impl Clock {
    fn new(r: &Report, now: Instant) -> Self {
        let age = r.age.unwrap_or_default();
        Self {
            reported: r.position,
            updated: r.updated,
            playing: r.playing,
            looked: now,
            position: r.position,
            sampled_at: now.checked_sub(age).unwrap_or(now),
        }
    }

    fn update(&mut self, r: &Report, now: Instant) {
        if r.updated != self.updated {
            // It's from since the last look; if it says otherwise, the system's clock
            // was changed.
            let since = now.saturating_duration_since(self.looked);
            let age = r.age.filter(|age| *age <= since + POLL);
            self.position = r.position;
            self.sampled_at = now.checked_sub(age.unwrap_or_default()).unwrap_or(now);
        } else if r.position != self.reported {
            // A new position with the old time.
            self.position = r.position;
            self.sampled_at = now;
        } else if r.playing != self.playing {
            // Paused or resumed without a new position.
            if self.playing {
                self.position += now.saturating_duration_since(self.sampled_at);
            }
            self.sampled_at = now;
        }
        self.reported = r.position;
        self.updated = r.updated;
        self.playing = r.playing;
        self.looked = now;
    }
}

/// The current song's cover.
#[derive(Default)]
struct Cover {
    /// Player, title, artist and album of the song it's for.
    song: [String; 4],
    /// Since when that's the song.
    since: Option<Instant>,
    file: Option<Arc<[u8]>>,
}

impl Cover {
    fn of(&mut self, song: [String; 4], from: &Properties, now: Instant) -> Option<Artwork> {
        if self.song != song {
            self.song = song;
            self.since = Some(now);
        }
        let settling = self
            .since
            .is_some_and(|since| now.duration_since(since) < COVER_SETTLES);
        if settling || self.file.is_none() {
            let file = read_cover(from).ok().flatten();
            // The same one again stays the same one, so it isn't decoded again.
            if self.file.as_deref() != file.as_deref() {
                self.file = file.map(Arc::from);
            }
        }
        self.file.clone().map(Artwork::File)
    }
}

/// Read the cover's file, if there is one.
fn read_cover(properties: &Properties) -> Result<Option<Vec<u8>>> {
    let Ok(thumbnail) = properties.Thumbnail() else {
        return Ok(None);
    };
    let stream = thumbnail.OpenReadAsync()?.join()?;
    let size = stream.Size()?;
    if size == 0 || size > MAX_COVER {
        return Ok(None);
    }
    let reader = DataReader::CreateDataReader(&stream)?;
    let loaded = reader.LoadAsync(size as u32)?.join()?;
    let mut bytes = vec![0; loaded as usize];
    reader.ReadBytes(&mut bytes)?;
    Ok(Some(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn s(seconds: u64) -> Duration {
        Duration::from_secs(seconds)
    }

    fn report(position: u64, updated: i64, age: Option<u64>, playing: bool) -> Report {
        Report {
            position: s(position),
            updated,
            age: age.map(s),
            playing,
        }
    }

    /// Where the song is at `now`, as [`Track::position_at`] works it out.
    fn at(c: &Clock, now: Instant) -> Duration {
        if c.playing {
            c.position + now.saturating_duration_since(c.sampled_at)
        } else {
            c.position
        }
    }

    #[test]
    fn positions_are_followed_between_reports() {
        let t0 = Instant::now();
        // Reported three seconds before the first look.
        let mut c = Clock::new(&report(10, 1, Some(3), true), t0);
        assert_eq!(at(&c, t0), s(13));
        // Nothing new: it goes on from there.
        c.update(&report(10, 1, Some(5), true), t0 + s(2));
        assert_eq!(at(&c, t0 + s(2)), s(15));
        // A seek.
        c.update(&report(60, 2, Some(1), true), t0 + s(4));
        assert_eq!(at(&c, t0 + s(4)), s(61));
    }

    #[test]
    fn pausing_without_a_new_position_stays_put() {
        let t0 = Instant::now();
        let mut c = Clock::new(&report(10, 1, Some(0), true), t0);
        c.update(&report(10, 1, Some(4), false), t0 + s(4));
        assert_eq!(at(&c, t0 + s(4)), s(14));
        c.update(&report(10, 1, Some(9), false), t0 + s(9));
        assert_eq!(at(&c, t0 + s(9)), s(14));
        c.update(&report(10, 1, Some(10), true), t0 + s(10));
        assert_eq!(at(&c, t0 + s(12)), s(16));
    }

    #[test]
    fn impossible_times_are_ignored() {
        let t0 = Instant::now();
        // Never set, or in the future.
        let mut c = Clock::new(&report(10, 0, None, true), t0);
        assert_eq!(at(&c, t0), s(10));
        // A new position, but its time stands still.
        c.update(&report(30, 0, Some(5000), true), t0 + s(1));
        assert_eq!(at(&c, t0 + s(1)), s(30));
        // The system's clock jumped ahead by an hour.
        c.update(&report(40, 7, Some(3600), true), t0 + s(2));
        assert_eq!(at(&c, t0 + s(2)), s(40));
    }

    #[test]
    fn system_times() {
        assert_eq!(duration(15_000_000), Duration::from_millis(1500));
        assert_eq!(duration(-1), Duration::ZERO);
        assert_eq!(age(0), None);
        assert_eq!(age(i64::MAX), None);
        assert_eq!(age(i64::MIN), None);
        let unix = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let ticks = 116_444_736_000_000_000 + (unix.as_nanos() / 100) as i64;
        let two_seconds_ago = age(ticks - 20_000_000).unwrap();
        assert!(two_seconds_ago >= s(2) && two_seconds_ago < s(10));
    }
}
