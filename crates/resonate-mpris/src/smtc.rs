use std::{
    os::windows::ffi::OsStrExt,
    sync::Arc,
    time::{Duration, Instant},
};

use resonate_core::MediaLocation;
use windows::{
    Foundation::{TimeSpan, TypedEventHandler, Uri},
    Media::{
        AutoRepeatModeChangeRequestedEventArgs, MediaPlaybackAutoRepeatMode, MediaPlaybackStatus,
        MediaPlaybackType, Playback::MediaPlayer, PlaybackPositionChangeRequestedEventArgs,
        ShuffleEnabledChangeRequestedEventArgs, SystemMediaTransportControls,
        SystemMediaTransportControlsButtonPressedEventArgs,
        SystemMediaTransportControlsTimelineProperties,
    },
    Storage::{StorageFile, Streams::RandomAccessStreamReference},
    core::HSTRING,
};

use crate::{
    Error, Result, WindowsOp,
    interfaces::{PlayerInterface, Shared},
    track::{loop_status, track_path},
    transport::{
        Controls, Displayed, Moved, Pressed, Status, Timeline, micros_of_ticks, repeat_asked,
        repeat_code,
    },
};

const TIMELINE_EVERY: Duration = Duration::from_secs(1);

pub(crate) struct Transport {
    _player: MediaPlayer,
    controls: SystemMediaTransportControls,
    told: Controls,
    timed: Instant,
    registrations: Registrations,
}

struct Registrations {
    pressed: i64,
    moved: i64,
    shuffled: i64,
    repeated: i64,
}

impl Transport {
    pub(crate) fn open(shared: &Arc<Shared>) -> Result<Self> {
        let player =
            MediaPlayer::new().map_err(|source| Error::windows(WindowsOp::Player, source))?;
        player
            .CommandManager()
            .and_then(|commands| commands.SetIsEnabled(false))
            .map_err(|source| Error::windows(WindowsOp::Player, source))?;
        let controls = player
            .SystemMediaTransportControls()
            .map_err(|source| Error::windows(WindowsOp::Controls, source))?;
        controls
            .SetIsEnabled(true)
            .map_err(|source| Error::windows(WindowsOp::Controls, source))?;
        let registrations = answer(&controls, shared)?;

        Ok(Self {
            _player: player,
            controls,
            told: Controls::default(),
            timed: Instant::now(),
            registrations,
        })
    }

    pub(crate) fn tell(&mut self, now: Controls, seeked: bool) {
        let due = self.timed.elapsed() >= TIMELINE_EVERY;
        let moved = Moved::between(&self.told, &now, seeked, due);

        if moved.status
            && let Some(status) = now.status
        {
            report(self.status(status));
        }
        if moved.buttons {
            report(self.buttons(&now));
        }
        if moved.modes {
            report(self.modes(&now));
        }
        if moved.displayed {
            report(self.displayed(now.displayed.as_ref()));
        }
        if moved.timeline {
            report(self.timeline(now.timeline));
            self.timed = Instant::now();
        }
        self.told = now;
    }

    fn status(&self, status: Status) -> Result<()> {
        self.controls
            .SetPlaybackStatus(MediaPlaybackStatus(status.code()))
            .map_err(|source| Error::windows(WindowsOp::Controls, source))
    }

    fn buttons(&self, now: &Controls) -> Result<()> {
        let buttons = now.buttons;
        let controls = &self.controls;

        controls
            .SetIsPlayEnabled(buttons.play)
            .and_then(|()| controls.SetIsPauseEnabled(buttons.pause))
            .and_then(|()| controls.SetIsStopEnabled(buttons.stop))
            .and_then(|()| controls.SetIsNextEnabled(buttons.next))
            .and_then(|()| controls.SetIsPreviousEnabled(buttons.previous))
            .map_err(|source| Error::windows(WindowsOp::Controls, source))
    }

    fn modes(&self, now: &Controls) -> Result<()> {
        self.controls
            .SetShuffleEnabled(now.shuffle)
            .map_err(|source| Error::windows(WindowsOp::Controls, source))?;
        let Some(repeat) = now.repeat else {
            return Ok(());
        };
        self.controls
            .SetAutoRepeatMode(MediaPlaybackAutoRepeatMode(repeat_code(repeat)))
            .map_err(|source| Error::windows(WindowsOp::Controls, source))
    }

    fn displayed(&self, displayed: Option<&Displayed>) -> Result<()> {
        let updater = self
            .controls
            .DisplayUpdater()
            .map_err(|source| Error::windows(WindowsOp::Display, source))?;
        updater
            .ClearAll()
            .map_err(|source| Error::windows(WindowsOp::Display, source))?;

        if let Some(displayed) = displayed {
            updater
                .SetType(MediaPlaybackType::Music)
                .map_err(|source| Error::windows(WindowsOp::Display, source))?;
            let music = updater
                .MusicProperties()
                .map_err(|source| Error::windows(WindowsOp::Display, source))?;
            music
                .SetTitle(&HSTRING::from(displayed.title.as_str()))
                .and_then(|()| music.SetArtist(&HSTRING::from(displayed.artist.as_str())))
                .and_then(|()| music.SetAlbumTitle(&HSTRING::from(displayed.album.as_str())))
                .and_then(|()| {
                    music.SetAlbumArtist(&HSTRING::from(displayed.album_artist.as_str()))
                })
                .and_then(|()| music.SetTrackNumber(displayed.track_number))
                .map_err(|source| Error::windows(WindowsOp::Display, source))?;
            if let Some(art) = displayed.art.as_deref() {
                match cover(art) {
                    Ok(cover) => updater
                        .SetThumbnail(&cover)
                        .map_err(|source| Error::windows(WindowsOp::Cover, source))?,
                    Err(error) => {
                        tracing::debug!(%error, "the cover did not reach the media controls");
                    }
                }
            }
        }

        updater
            .Update()
            .map_err(|source| Error::windows(WindowsOp::Display, source))
    }

    fn timeline(&self, timeline: Option<Timeline>) -> Result<()> {
        let timeline = timeline.unwrap_or(Timeline {
            position: 0,
            end: 0,
            seekable: false,
        });
        let properties = SystemMediaTransportControlsTimelineProperties::new()
            .map_err(|source| Error::windows(WindowsOp::Timeline, source))?;

        properties
            .SetStartTime(span(0))
            .and_then(|()| properties.SetEndTime(span(timeline.end)))
            .and_then(|()| properties.SetMinSeekTime(span(0)))
            .and_then(|()| properties.SetMaxSeekTime(span(timeline.seek_end())))
            .and_then(|()| properties.SetPosition(span(timeline.position)))
            .and_then(|()| self.controls.UpdateTimelineProperties(&properties))
            .map_err(|source| Error::windows(WindowsOp::Timeline, source))
    }

    pub(crate) fn close(self) {
        let controls = &self.controls;
        let registrations = &self.registrations;
        let left = controls
            .RemoveButtonPressed(registrations.pressed)
            .and_then(|()| controls.RemovePlaybackPositionChangeRequested(registrations.moved))
            .and_then(|()| controls.RemoveShuffleEnabledChangeRequested(registrations.shuffled))
            .and_then(|()| controls.RemoveAutoRepeatModeChangeRequested(registrations.repeated))
            .map_err(|source| Error::windows(WindowsOp::Listen, source));
        report(left);
        report(self.status(Status::Closed));
        report(self.displayed(None));
        report(
            controls
                .SetIsEnabled(false)
                .map_err(|source| Error::windows(WindowsOp::Controls, source)),
        );
    }
}

fn answer(controls: &SystemMediaTransportControls, shared: &Arc<Shared>) -> Result<Registrations> {
    let player = Arc::new(PlayerInterface {
        shared: Arc::clone(shared),
    });

    let pressing = Arc::clone(&player);
    let pressed = controls
        .ButtonPressed(&TypedEventHandler::<
            SystemMediaTransportControls,
            SystemMediaTransportControlsButtonPressedEventArgs,
        >::new(move |_, arguments| {
            let pressed = arguments
                .as_ref()
                .and_then(|arguments| arguments.Button().ok())
                .and_then(|button| Pressed::of(button.0));
            if let Some(pressed) = pressed {
                refused(answered(&pressing, pressed));
            }
            Ok(())
        }))
        .map_err(|source| Error::windows(WindowsOp::Listen, source))?;

    let seeking = Arc::clone(&player);
    let moved = controls
        .PlaybackPositionChangeRequested(&TypedEventHandler::<
            SystemMediaTransportControls,
            PlaybackPositionChangeRequestedEventArgs,
        >::new(move |_, arguments| {
            let asked = arguments
                .as_ref()
                .and_then(|arguments| arguments.RequestedPlaybackPosition().ok());
            if let Some(asked) = asked {
                refused(moved_to(&seeking, asked.Duration));
            }
            Ok(())
        }))
        .map_err(|source| Error::windows(WindowsOp::Listen, source))?;

    let shuffling = Arc::clone(&player);
    let shuffled = controls
        .ShuffleEnabledChangeRequested(&TypedEventHandler::<
            SystemMediaTransportControls,
            ShuffleEnabledChangeRequestedEventArgs,
        >::new(move |_, arguments| {
            let asked = arguments
                .as_ref()
                .and_then(|arguments| arguments.RequestedShuffleEnabled().ok());
            if let Some(asked) = asked {
                refused(shuffling.set_shuffle(asked));
            }
            Ok(())
        }))
        .map_err(|source| Error::windows(WindowsOp::Listen, source))?;

    let repeating = player;
    let repeated = controls
        .AutoRepeatModeChangeRequested(&TypedEventHandler::<
            SystemMediaTransportControls,
            AutoRepeatModeChangeRequestedEventArgs,
        >::new(move |_, arguments| {
            let asked = arguments
                .as_ref()
                .and_then(|arguments| arguments.RequestedAutoRepeatMode().ok())
                .and_then(|mode| repeat_asked(mode.0));
            if let Some(repeat) = asked {
                refused(repeating.set_loop_status(loop_status(repeat)));
            }
            Ok(())
        }))
        .map_err(|source| Error::windows(WindowsOp::Listen, source))?;

    Ok(Registrations {
        pressed,
        moved,
        shuffled,
        repeated,
    })
}

fn answered(player: &PlayerInterface, pressed: Pressed) -> zbus::fdo::Result<()> {
    match pressed {
        Pressed::Play => player.play(),
        Pressed::Pause => player.pause(),
        Pressed::Stop => player.stop(),
        Pressed::Next => player.next(),
        Pressed::Previous => player.previous(),
    }
}

fn moved_to(player: &PlayerInterface, ticks: i64) -> zbus::fdo::Result<()> {
    let Some(current) = player.shared.player.state().current else {
        return Ok(());
    };
    player.set_position(track_path(current.id).into_inner(), micros_of_ticks(ticks))
}

fn refused(outcome: zbus::fdo::Result<()>) {
    if let Err(error) = outcome {
        tracing::debug!(%error, "the media controls asked for what the player refused");
    }
}

fn cover(art: &str) -> Result<RandomAccessStreamReference> {
    let path = MediaLocation::from_uri(art).and_then(|location| {
        location.as_path().map(|path| {
            resonate_core::path_text::plain(path)
                .as_os_str()
                .encode_wide()
                .collect::<Vec<u16>>()
        })
    });

    match path {
        Some(wide) => {
            let file = StorageFile::GetFileFromPathAsync(&HSTRING::from_wide(&wide))
                .and_then(|opening| opening.join())
                .map_err(|source| Error::windows(WindowsOp::Cover, source))?;
            RandomAccessStreamReference::CreateFromFile(&file)
                .map_err(|source| Error::windows(WindowsOp::Cover, source))
        }
        None => {
            let uri = Uri::CreateUri(&HSTRING::from(art))
                .map_err(|source| Error::windows(WindowsOp::Cover, source))?;
            RandomAccessStreamReference::CreateFromUri(&uri)
                .map_err(|source| Error::windows(WindowsOp::Cover, source))
        }
    }
}

const fn span(ticks: i64) -> TimeSpan {
    TimeSpan { Duration: ticks }
}

fn report(outcome: Result<()>) {
    if let Err(error) = outcome {
        tracing::debug!(%error, "the media controls were not told what plays");
    }
}
