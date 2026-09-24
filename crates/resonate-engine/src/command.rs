use std::{fmt, sync::Arc, time::Duration};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError, bounded};
use resonate_core::{Frames, Resumption, Span, Volume};
use resonate_dsp::{DitherKind, FilterPhase, NoiseShaping, Quality, ReplayGainMode, Restoration};
use resonate_pipewire::NodeName;

use crate::{BluetoothWake, Equalisation, Error, Levelling, Placement, QueueItem, Result, Until};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RepeatMode {
    #[default]
    Off,
    Track,
    Queue,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SkipUnderRepeat {
    #[default]
    RepeatsTheQueue,
    KeepsRepeatingTheTrack,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Load {
        items: Vec<QueueItem>,
        start_at: usize,
        autoplay: bool,
    },
    Resume(Resumption),
    Insert {
        items: Vec<QueueItem>,
        at: Placement,
        play: bool,
    },
    Remove(Span),
    Move {
        rows: Span,
        to: usize,
    },
    Order(Vec<usize>),
    Play,
    Pause,
    TogglePlayPause,
    Stop,
    Seek(Frames),
    SeekBy(i64),
    Next,
    Previous,
    JumpTo(usize),
    SetVolume(Volume),
    SetRepeat(RepeatMode),
    SetSkipUnderRepeat(SkipUnderRepeat),
    SetShuffle(bool),
    SetSink(Option<NodeName>),
    SetQuality(Quality),
    SetFilterPhase(FilterPhase),
    SetTruePeak(bool),
    SetRestoration(Restoration),
    SetDither(DitherKind),
    SetNoiseShaping(NoiseShaping),
    SetReplayGain(ReplayGainMode),
    SetLevelling(Levelling),
    SetEqualisation(Arc<Equalisation>),
    SetBitPerfect(bool),
    SetDop(bool),
    SetForceGraphRate(bool),
    SetBluetoothWake(BluetoothWake),
    SetBuffer(Duration),
    SleepUntil(Option<Until>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandKind {
    Load,
    Resume,
    Insert,
    Remove,
    Move,
    Order,
    Play,
    Pause,
    TogglePlayPause,
    Stop,
    Seek,
    SeekBy,
    Next,
    Previous,
    JumpTo,
    SetVolume,
    SetRepeat,
    SetSkipUnderRepeat,
    SetShuffle,
    SetSink,
    SetQuality,
    SetFilterPhase,
    SetTruePeak,
    SetRestoration,
    SetDither,
    SetNoiseShaping,
    SetReplayGain,
    SetLevelling,
    SetEqualisation,
    SetBitPerfect,
    SetDop,
    SetForceGraphRate,
    SetBluetoothWake,
    SetBuffer,
    SleepUntil,
}

impl CommandKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Load => "the load",
            Self::Resume => "the resumption",
            Self::Insert => "the insert",
            Self::Remove => "the removal",
            Self::Move => "the move",
            Self::Order => "the reordering",
            Self::Play => "play",
            Self::Pause => "pause",
            Self::TogglePlayPause => "play/pause",
            Self::Stop => "stop",
            Self::Seek | Self::SeekBy => "the seek",
            Self::Next => "next",
            Self::Previous => "previous",
            Self::JumpTo => "the jump",
            Self::SetVolume => "the volume",
            Self::SetRepeat => "repeat",
            Self::SetSkipUnderRepeat => "what a skip does to repeat",
            Self::SetShuffle => "shuffle",
            Self::SetSink => "the device",
            Self::SetQuality => "the resampler",
            Self::SetFilterPhase => "the resampler's phase",
            Self::SetTruePeak => "true-peak clip prevention",
            Self::SetRestoration => "lossy restoration",
            Self::SetDither => "dither",
            Self::SetNoiseShaping => "noise shaping",
            Self::SetReplayGain => "ReplayGain",
            Self::SetLevelling => "the pre-amp",
            Self::SetEqualisation => "the equaliser",
            Self::SetBitPerfect => "the sample rate",
            Self::SetDop => "DSD over PCM",
            Self::SetForceGraphRate => "the graph rate",
            Self::SetBluetoothWake => "keeping Bluetooth awake",
            Self::SetBuffer => "the buffer",
            Self::SleepUntil => "the sleep timer",
        }
    }
}

impl fmt::Display for CommandKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Command {
    pub const fn kind(&self) -> CommandKind {
        match self {
            Self::Load { .. } => CommandKind::Load,
            Self::Resume(_) => CommandKind::Resume,
            Self::Insert { .. } => CommandKind::Insert,
            Self::Remove(_) => CommandKind::Remove,
            Self::Move { .. } => CommandKind::Move,
            Self::Order(_) => CommandKind::Order,
            Self::Play => CommandKind::Play,
            Self::Pause => CommandKind::Pause,
            Self::TogglePlayPause => CommandKind::TogglePlayPause,
            Self::Stop => CommandKind::Stop,
            Self::Seek(_) => CommandKind::Seek,
            Self::SeekBy(_) => CommandKind::SeekBy,
            Self::Next => CommandKind::Next,
            Self::Previous => CommandKind::Previous,
            Self::JumpTo(_) => CommandKind::JumpTo,
            Self::SetVolume(_) => CommandKind::SetVolume,
            Self::SetRepeat(_) => CommandKind::SetRepeat,
            Self::SetSkipUnderRepeat(_) => CommandKind::SetSkipUnderRepeat,
            Self::SetShuffle(_) => CommandKind::SetShuffle,
            Self::SetSink(_) => CommandKind::SetSink,
            Self::SetQuality(_) => CommandKind::SetQuality,
            Self::SetFilterPhase(_) => CommandKind::SetFilterPhase,
            Self::SetTruePeak(_) => CommandKind::SetTruePeak,
            Self::SetRestoration(_) => CommandKind::SetRestoration,
            Self::SetDither(_) => CommandKind::SetDither,
            Self::SetNoiseShaping(_) => CommandKind::SetNoiseShaping,
            Self::SetReplayGain(_) => CommandKind::SetReplayGain,
            Self::SetLevelling(_) => CommandKind::SetLevelling,
            Self::SetEqualisation(_) => CommandKind::SetEqualisation,
            Self::SetBitPerfect(_) => CommandKind::SetBitPerfect,
            Self::SetDop(_) => CommandKind::SetDop,
            Self::SetForceGraphRate(_) => CommandKind::SetForceGraphRate,
            Self::SetBluetoothWake(_) => CommandKind::SetBluetoothWake,
            Self::SetBuffer(_) => CommandKind::SetBuffer,
            Self::SleepUntil(_) => CommandKind::SleepUntil,
        }
    }
}

pub(crate) struct Request {
    pub command: Command,
    pub reply: Reply,
}

pub(crate) enum Reply {
    Unwaited,
    Answered(Sender<Result<()>>),
    Landed(Sender<()>),
}

impl Request {
    pub(crate) const fn told(command: Command) -> Self {
        Self {
            command,
            reply: Reply::Unwaited,
        }
    }

    pub(crate) fn asked(command: Command) -> (Self, Outcome) {
        let kind = command.kind();
        let (sender, reply) = bounded(1);

        (
            Self {
                command,
                reply: Reply::Answered(sender),
            },
            Outcome { kind, reply },
        )
    }

    pub(crate) fn settled(command: Command) -> (Self, Landing) {
        let kind = command.kind();
        let (sender, landed) = bounded(1);

        (
            Self {
                command,
                reply: Reply::Landed(sender),
            },
            Landing { kind, landed },
        )
    }
}

pub struct Outcome {
    kind: CommandKind,
    reply: Receiver<Result<()>>,
}

impl Outcome {
    pub const fn command(&self) -> CommandKind {
        self.kind
    }

    pub fn poll(&self) -> Option<Result<()>> {
        match self.reply.try_recv() {
            Ok(outcome) => Some(outcome),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err(Error::EngineStopped)),
        }
    }

    pub fn wait(self) -> Result<()> {
        self.reply.recv().unwrap_or(Err(Error::EngineStopped))
    }

    pub fn wait_for(self, timeout: Duration) -> Result<()> {
        match self.reply.recv_timeout(timeout) {
            Ok(outcome) => outcome,
            Err(RecvTimeoutError::Timeout) => Err(Error::CommandPending { command: self.kind }),
            Err(RecvTimeoutError::Disconnected) => Err(Error::EngineStopped),
        }
    }
}

pub struct Landing {
    kind: CommandKind,
    landed: Receiver<()>,
}

impl Landing {
    pub fn wait_for(self, timeout: Duration) -> Result<()> {
        match self.landed.recv_timeout(timeout) {
            Ok(()) => Ok(()),
            Err(RecvTimeoutError::Timeout) => Err(Error::CommandPending { command: self.kind }),
            Err(RecvTimeoutError::Disconnected) => Err(Error::EngineStopped),
        }
    }
}
