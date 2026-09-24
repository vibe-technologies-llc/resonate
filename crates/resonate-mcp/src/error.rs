use std::{fmt, io, path::PathBuf, result};

use resonate_core::TrackId;
use resonate_library::PlaylistName;
use resonate_mpris::PlayerName;
use thiserror::Error;

use crate::{passes::Pass, tools::Tool};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StreamOp {
    Read,
    Write,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("{op:?} failed on the protocol stream")]
    Stream {
        op: StreamOp,
        #[source]
        source: io::Error,
    },

    #[error("no player is on the session bus; start one with resonate play or open the window")]
    NothingRunning,

    #[error("no player named {name} is on the session bus")]
    NoSuchPlayer { name: PlayerName },

    #[error("no playlist named {0}")]
    NoSuchPlaylist(PlaylistName),

    #[error("the running player holds no playlist named {0}")]
    PlaylistNotOffered(PlaylistName),

    #[error("the catalog holds no track numbered {track}")]
    NoSuchTrack { track: TrackId },

    #[error("{playlist} holds no row {row}")]
    NotInThePlaylist { playlist: PlaylistName, row: usize },

    #[error("the queue holds no row numbered {track}")]
    NotInTheQueue { track: TrackId },

    #[error("a {pass} this session started is still running")]
    AlreadyRunning { pass: Pass },

    #[error(
        "nothing here can look anything up: this build carries no online lookup, or online is off"
    )]
    NoReference,

    #[error("{path} is not a folder that can be scanned", path = path.display())]
    NoSuchFolder { path: PathBuf },

    #[error(transparent)]
    Core(#[from] resonate_core::Error),

    #[error(transparent)]
    Library(#[from] resonate_library::Error),

    #[error(transparent)]
    Mpris(#[from] resonate_mpris::Error),
}

pub type Result<T> = result::Result<T, Error>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MethodName(Box<str>);

impl MethodName {
    pub fn new(name: impl Into<Box<str>>) -> Self {
        Self(name.into())
    }
}

impl fmt::Display for MethodName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ToolName(Box<str>);

impl ToolName {
    pub fn new(name: impl Into<Box<str>>) -> Self {
        Self(name.into())
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResourceUri(Box<str>);

impl ResourceUri {
    pub fn new(uri: impl Into<Box<str>>) -> Self {
        Self(uri.into())
    }
}

impl fmt::Display for ResourceUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Error)]
pub enum Refusal {
    #[error("the message is not JSON")]
    Unparsed(#[source] serde_json::Error),

    #[error("the message is not a JSON-RPC 2.0 request or notification")]
    NotARequest,

    #[error("{0} is not a method this server answers")]
    UnknownMethod(MethodName),

    #[error("the parameters of {method} are not what it takes")]
    BadParameters {
        method: MethodName,
        #[source]
        source: serde_json::Error,
    },

    #[error("{0} is not a tool this server offers")]
    UnknownTool(ToolName),

    #[error("{0} is not a resource this server offers")]
    UnknownResource(ResourceUri),

    #[error("the arguments of {tool} are not what it takes")]
    BadArguments {
        tool: Tool,
        #[source]
        source: serde_json::Error,
    },

    #[error("{tool} takes exactly one of {}", fields.join(", "))]
    OneOf {
        tool: Tool,
        fields: &'static [&'static str],
    },

    #[error("{tool} takes at least one of {}", fields.join(", "))]
    AtLeastOneOf {
        tool: Tool,
        fields: &'static [&'static str],
    },

    #[error("{tool} takes no more than one of {}", fields.join(", "))]
    AtMostOneOf {
        tool: Tool,
        fields: &'static [&'static str],
    },

    #[error("{field} is outside the range {tool} takes")]
    OutOfRange { tool: Tool, field: &'static str },

    #[error("{field} is not a value {tool} can read")]
    Unreadable { tool: Tool, field: &'static str },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Code {
    ParseError,
    InvalidRequest,
    MethodNotFound,
    InvalidParams,
    InternalError,
    ResourceNotFound,
}

impl Code {
    pub const fn number(self) -> i64 {
        match self {
            Self::ParseError => -32_700,
            Self::InvalidRequest => -32_600,
            Self::MethodNotFound => -32_601,
            Self::InvalidParams => -32_602,
            Self::InternalError => -32_603,
            Self::ResourceNotFound => -32_002,
        }
    }
}

impl Refusal {
    pub const fn code(&self) -> Code {
        match self {
            Self::Unparsed(_) => Code::ParseError,
            Self::NotARequest => Code::InvalidRequest,
            Self::UnknownMethod(_) => Code::MethodNotFound,
            Self::UnknownResource(_) => Code::ResourceNotFound,
            Self::BadParameters { .. }
            | Self::UnknownTool(_)
            | Self::BadArguments { .. }
            | Self::OneOf { .. }
            | Self::AtLeastOneOf { .. }
            | Self::AtMostOneOf { .. }
            | Self::OutOfRange { .. }
            | Self::Unreadable { .. } => Code::InvalidParams,
        }
    }
}

pub(crate) fn said(error: &(dyn std::error::Error + 'static)) -> String {
    let mut said = error.to_string();
    let mut cause = error.source();
    while let Some(reason) = cause {
        said.push_str(": ");
        said.push_str(&reason.to_string());
        cause = reason.source();
    }
    said
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_stays_small_enough_for_every_result_in_the_crate() {
        assert!(size_of::<Error>() <= 128, "{}", size_of::<Error>());
        assert!(size_of::<Refusal>() <= 128, "{}", size_of::<Refusal>());
    }

    #[test]
    fn a_refusal_carries_the_code_json_rpc_gives_its_kind() {
        assert_eq!(Refusal::NotARequest.code().number(), -32_600);
        assert_eq!(
            Refusal::UnknownMethod(MethodName::new("prompts/list"))
                .code()
                .number(),
            -32_601
        );
        assert_eq!(
            Refusal::UnknownTool(ToolName::new("launch"))
                .code()
                .number(),
            -32_602
        );
    }
}
