mod catalog;
mod completions;
mod controlling;
mod edits;
mod error;
mod passes;
mod prompts;
mod resources;
mod server;
mod tools;
mod transport;
mod written;

#[cfg(fuzzing)]
pub fn read_the_lines(bytes: &[u8]) {
    crate::server::read_every_line(bytes);
}

pub use crate::{
    controlling::{Controlling, OnTheBus, Reach, Row},
    error::{
        ArgumentName, Code, Error, MethodName, PromptName, Refusal, ResourceUri, Result, StreamOp,
        ToolName,
    },
    passes::{Lookups, Pass},
    prompts::Prompt,
    resources::Resource,
    server::{Server, Stop, Stoppable, stoppable},
    tools::Tool,
};
