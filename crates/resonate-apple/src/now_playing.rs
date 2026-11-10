use std::{ptr::NonNull, sync::Arc, time::Duration};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2::{AnyThread, rc::Retained, runtime::AnyObject};
use objc2_app_kit::NSImage;
use objc2_core_foundation::CGSize;
use objc2_foundation::{NSData, NSMutableCopying, NSMutableDictionary, NSNumber, NSString};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPMediaItemArtwork, MPMediaItemPropertyAlbumArtist,
    MPMediaItemPropertyAlbumTitle, MPMediaItemPropertyAlbumTrackNumber, MPMediaItemPropertyArtist,
    MPMediaItemPropertyArtwork, MPMediaItemPropertyPlaybackDuration, MPMediaItemPropertyTitle,
    MPNowPlayingInfoCenter, MPNowPlayingInfoPropertyElapsedPlaybackTime,
    MPNowPlayingInfoPropertyPlaybackRate, MPNowPlayingPlaybackState, MPRemoteCommand,
    MPRemoteCommandCenter, MPRemoteCommandEvent, MPRemoteCommandHandlerStatus,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub track_number: u32,
    pub cover: Option<Arc<[u8]>>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clock {
    pub elapsed: Duration,
    pub length: Option<Duration>,
    pub rate: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Playback {
    Playing,
    Paused,
    Stopped,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offered {
    pub play: bool,
    pub pause: bool,
    pub stop: bool,
    pub next: bool,
    pub previous: bool,
    pub seek: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    Previous,
    Seek(Duration),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Answer {
    Done,
    Failed,
    NothingPlaying,
}

impl Answer {
    const fn status(self) -> MPRemoteCommandHandlerStatus {
        match self {
            Self::Done => MPRemoteCommandHandlerStatus::Success,
            Self::Failed => MPRemoteCommandHandlerStatus::CommandFailed,
            Self::NothingPlaying => MPRemoteCommandHandlerStatus::NoActionableNowPlayingItem,
        }
    }
}

type Asking = Arc<dyn Fn(Asked) -> Answer + Send + Sync>;

#[derive(Clone, Copy)]
enum Pressed {
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    Previous,
}

impl Pressed {
    const EVERY: [Self; 6] = [
        Self::Play,
        Self::Pause,
        Self::Toggle,
        Self::Stop,
        Self::Next,
        Self::Previous,
    ];

    const fn asked(self) -> Asked {
        match self {
            Self::Play => Asked::Play,
            Self::Pause => Asked::Pause,
            Self::Toggle => Asked::Toggle,
            Self::Stop => Asked::Stop,
            Self::Next => Asked::Next,
            Self::Previous => Asked::Previous,
        }
    }

    fn command(self, center: &MPRemoteCommandCenter) -> Retained<MPRemoteCommand> {
        match self {
            Self::Play => unsafe { center.playCommand() },
            Self::Pause => unsafe { center.pauseCommand() },
            Self::Toggle => unsafe { center.togglePlayPauseCommand() },
            Self::Stop => unsafe { center.stopCommand() },
            Self::Next => unsafe { center.nextTrackCommand() },
            Self::Previous => unsafe { center.previousTrackCommand() },
        }
    }

    const fn is_offered(self, offered: Offered) -> bool {
        match self {
            Self::Play => offered.play,
            Self::Pause => offered.pause,
            Self::Toggle => offered.play || offered.pause,
            Self::Stop => offered.stop,
            Self::Next => offered.next,
            Self::Previous => offered.previous,
        }
    }
}

fn on_the_main_queue(work: impl FnOnce() + Send + 'static) {
    DispatchQueue::main().exec_async(work);
}

fn commands() -> Retained<MPRemoteCommandCenter> {
    unsafe { MPRemoteCommandCenter::sharedCommandCenter() }
}

fn info_center() -> Retained<MPNowPlayingInfoCenter> {
    unsafe { MPNowPlayingInfoCenter::defaultCenter() }
}

fn answered(
    command: &MPRemoteCommand,
    answer: impl Fn(NonNull<MPRemoteCommandEvent>) -> Answer + 'static,
) {
    let handler = RcBlock::new(move |event: NonNull<MPRemoteCommandEvent>| answer(event).status());
    let _target = unsafe { command.addTargetWithHandler(&handler) };
}

fn position_asked(event: NonNull<MPRemoteCommandEvent>) -> Option<Duration> {
    let event = unsafe { event.as_ref() };
    let moved = event.downcast_ref::<MPChangePlaybackPositionCommandEvent>()?;
    let seconds = unsafe { moved.positionTime() };
    Duration::try_from_secs_f64(seconds).ok()
}

fn artwork_of(cover: &[u8]) -> Option<Retained<MPMediaItemArtwork>> {
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(cover))?;
    let bounds = image.size();
    let handler = RcBlock::new(move |_: CGSize| NonNull::from(&*image));
    Some(unsafe {
        MPMediaItemArtwork::initWithBoundsSize_requestHandler(
            MPMediaItemArtwork::alloc(),
            bounds,
            &handler,
        )
    })
}

type Info = NSMutableDictionary<NSString, AnyObject>;

struct Keys {
    title: &'static NSString,
    artist: &'static NSString,
    album: &'static NSString,
    album_artist: &'static NSString,
    track_number: &'static NSString,
    artwork: &'static NSString,
    length: &'static NSString,
    elapsed: &'static NSString,
    rate: &'static NSString,
}

impl Keys {
    fn of_the_system() -> Self {
        Self {
            title: unsafe { MPMediaItemPropertyTitle },
            artist: unsafe { MPMediaItemPropertyArtist },
            album: unsafe { MPMediaItemPropertyAlbumTitle },
            album_artist: unsafe { MPMediaItemPropertyAlbumArtist },
            track_number: unsafe { MPMediaItemPropertyAlbumTrackNumber },
            artwork: unsafe { MPMediaItemPropertyArtwork },
            length: unsafe { MPMediaItemPropertyPlaybackDuration },
            elapsed: unsafe { MPNowPlayingInfoPropertyElapsedPlaybackTime },
            rate: unsafe { MPNowPlayingInfoPropertyPlaybackRate },
        }
    }

    fn of_the_clock(&self) -> [&'static NSString; 3] {
        [self.length, self.elapsed, self.rate]
    }
}

fn named(info: &Info, key: &NSString, text: &str) {
    if !text.is_empty() {
        info.insert(key, &NSString::from_str(text));
    }
}

fn shown(shown: &Shown) {
    let keys = Keys::of_the_system();
    let center = info_center();
    let info = Info::new();
    named(&info, keys.title, &shown.title);
    named(&info, keys.artist, &shown.artist);
    named(&info, keys.album, &shown.album);
    named(&info, keys.album_artist, &shown.album_artist);
    if shown.track_number != 0 {
        info.insert(keys.track_number, &NSNumber::new_u32(shown.track_number));
    }
    if let Some(artwork) = shown.cover.as_deref().and_then(artwork_of) {
        info.insert(keys.artwork, &artwork);
    }
    if let Some(standing) = unsafe { center.nowPlayingInfo() } {
        for key in keys.of_the_clock() {
            if let Some(kept) = standing.objectForKey(key) {
                info.insert(key, &kept);
            }
        }
    }
    unsafe { center.setNowPlayingInfo(Some(&info)) };
}

fn timed(clock: Option<Clock>) {
    let keys = Keys::of_the_system();
    let center = info_center();
    let Some(standing) = (unsafe { center.nowPlayingInfo() }) else {
        return;
    };
    let info: Retained<Info> = standing.mutableCopy();
    for key in keys.of_the_clock() {
        info.removeObjectForKey(key);
    }
    if let Some(clock) = clock {
        info.insert(
            keys.elapsed,
            &NSNumber::new_f64(clock.elapsed.as_secs_f64()),
        );
        info.insert(keys.rate, &NSNumber::new_f64(clock.rate));
        if let Some(length) = clock.length {
            info.insert(keys.length, &NSNumber::new_f64(length.as_secs_f64()));
        }
    }
    unsafe { center.setNowPlayingInfo(Some(&info)) };
}

pub struct NowPlaying;

impl NowPlaying {
    pub fn answered_by(asked: impl Fn(Asked) -> Answer + Send + Sync + 'static) -> Self {
        let asked: Asking = Arc::new(asked);
        on_the_main_queue(move || {
            let center = commands();
            for pressed in Pressed::EVERY {
                let asked = Arc::clone(&asked);
                answered(&pressed.command(&center), move |_| asked(pressed.asked()));
            }
            let seek = unsafe { center.changePlaybackPositionCommand() };
            answered(&seek, move |event| match position_asked(event) {
                Some(position) => asked(Asked::Seek(position)),
                None => Answer::Failed,
            });
        });
        Self
    }

    pub fn show(&self, song: Option<&Shown>) {
        let song = song.cloned();
        on_the_main_queue(move || match song {
            Some(song) => shown(&song),
            None => unsafe { info_center().setNowPlayingInfo(None) },
        });
    }

    pub fn clock(&self, clock: Option<Clock>) {
        on_the_main_queue(move || timed(clock));
    }

    pub fn playback(&self, playback: Playback) {
        on_the_main_queue(move || {
            let state = match playback {
                Playback::Playing => MPNowPlayingPlaybackState::Playing,
                Playback::Paused => MPNowPlayingPlaybackState::Paused,
                Playback::Stopped => MPNowPlayingPlaybackState::Stopped,
            };
            unsafe { info_center().setPlaybackState(state) };
        });
    }

    pub fn offer(&self, offered: Offered) {
        on_the_main_queue(move || {
            let center = commands();
            for pressed in Pressed::EVERY {
                let command = pressed.command(&center);
                unsafe { command.setEnabled(pressed.is_offered(offered)) };
            }
            let seek = unsafe { center.changePlaybackPositionCommand() };
            unsafe { seek.setEnabled(offered.seek) };
        });
    }
}

impl Drop for NowPlaying {
    fn drop(&mut self) {
        on_the_main_queue(|| {
            let center = commands();
            for pressed in Pressed::EVERY {
                let command = pressed.command(&center);
                unsafe { command.removeTarget(None) };
            }
            let seek = unsafe { center.changePlaybackPositionCommand() };
            unsafe { seek.removeTarget(None) };
            let info = info_center();
            unsafe { info.setNowPlayingInfo(None) };
            unsafe { info.setPlaybackState(MPNowPlayingPlaybackState::Stopped) };
        });
    }
}
