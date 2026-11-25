//! Media players over MPRIS (Linux).

use super::{Artwork, Command, MediaTarget, Request, Shared, Track};
use mpris::{LoopStatus, PlaybackStatus, Player, PlayerFinder};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(500);

pub fn spawn(
    state: Arc<Mutex<Shared>>,
    mut target: MediaTarget,
    connection: u32,
) -> Option<Sender<Request>> {
    let (tx, rx) = mpsc::channel::<Request>();
    std::thread::Builder::new()
        .name("undavi-mpris".into())
        .spawn(move || {
            let finder = match PlayerFinder::new() {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("undavi: MPRIS unavailable: {e}");
                    return;
                }
            };
            loop {
                let player = resolve_player(&finder, &target);
                let snapshot = player.as_ref().and_then(read);
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
                        let mut active = player.or_else(|| resolve_player(&finder, &target));
                        if let Some(p) = &active {
                            apply(p, cmd);
                        }
                        while let Ok(msg) = rx.try_recv() {
                            match msg {
                                Request::Target(new_target) => {
                                    target = new_target;
                                    active = resolve_player(&finder, &target);
                                }
                                Request::Command(cmd) => {
                                    if let Some(p) = &active {
                                        apply(p, cmd);
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

fn resolve_player(finder: &PlayerFinder, target: &MediaTarget) -> Option<Player> {
    match target {
        MediaTarget::Any => finder.find_active().ok(),
        MediaTarget::App(_) => {
            let iter = finder.iter_players().ok()?;
            let mut best: Option<(u8, Player)> = None;
            for player in iter.flatten() {
                if !target.matches(player.bus_name_trimmed(), player.identity()) {
                    continue;
                }
                let rank = match player.get_playback_status() {
                    Ok(PlaybackStatus::Playing) => 0,
                    Ok(PlaybackStatus::Paused) => 1,
                    Ok(PlaybackStatus::Stopped) | Err(_) => 2,
                };
                if rank == 0 {
                    return Some(player);
                }
                if best.as_ref().is_none_or(|(best_rank, _)| rank < *best_rank) {
                    best = Some((rank, player));
                }
            }
            best.map(|(_, p)| p)
        }
    }
}

fn read(p: &Player) -> Option<Track> {
    let meta = p.get_metadata().ok()?;
    let status = p.get_playback_status().ok()?;
    Some(Track {
        title: meta.title().unwrap_or_default().to_string(),
        artist: meta.artists().map(|a| a.join(", ")).unwrap_or_default(),
        album: meta.album_name().unwrap_or_default().to_string(),
        art: meta
            .art_url()
            .filter(|u| !u.is_empty())
            .map(|u| Artwork::Url(u.to_string())),
        url: meta.url().filter(|u| !u.is_empty()).map(str::to_string),
        length: meta.length().filter(|d| !d.is_zero()),
        position: p.get_position().unwrap_or_default(),
        sampled_at: Instant::now(),
        playing: status == PlaybackStatus::Playing,
        repeat: matches!(
            p.get_loop_status(),
            Ok(LoopStatus::Track | LoopStatus::Playlist)
        ),
        volume: p.get_volume().ok(),
        can_seek: p.can_seek().unwrap_or(false),
        player_bus: p.bus_name_trimmed().to_string(),
        player_name: p.identity().to_string(),
    })
}

fn apply(p: &Player, cmd: Command) {
    let result = match cmd {
        Command::PlayPause => p.play_pause(),
        Command::Next => p.next(),
        Command::Previous => p.previous(),
        Command::SeekTo(pos) => match p.get_metadata().ok().and_then(|m| m.track_id()) {
            Some(id) => p.set_position(id, &pos),
            None => Ok(()),
        },
        Command::SetRepeat(on) => p.set_loop_status(if on {
            LoopStatus::Playlist
        } else {
            LoopStatus::None
        }),
        Command::SetVolume(v) => p.set_volume(v.clamp(0.0, 1.0)),
    };
    if let Err(e) = result {
        eprintln!("undavi: media command failed: {e}");
    }
}
