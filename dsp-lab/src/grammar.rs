use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "dsp-lab",
    about = "Measure Resonate's DSP stages and whole chains for quality and cost, against references"
)]
pub struct Grammar {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    #[command(about = "Name every suite, what it measures, its subjects and its rates")]
    List,
    #[command(about = "Measure quality and print each suite's tables")]
    Measure(Measuring),
    #[command(about = "Time throughput and construction; wants an idle machine")]
    Time(Timing),
    #[command(about = "Weigh resampler candidates from a file against the resample suite")]
    Candidates(Candidates),
}

#[derive(Debug, Args)]
pub struct Chosen {
    #[arg(help = "Suites to run, every one when none is named")]
    pub suites: Vec<String>,
    #[arg(long, help = "Keep only subjects whose name holds this text")]
    pub only: Option<String>,
}

#[derive(Debug, Args)]
pub struct Measuring {
    #[command(flatten)]
    pub chosen: Chosen,
    #[arg(
        long,
        value_name = "DIR",
        help = "Write each suite's plots into this folder"
    )]
    pub plots: Option<PathBuf>,
    #[arg(
        long,
        value_name = "FILE",
        help = "Write every reading as tab-separated lines"
    )]
    pub tsv: Option<PathBuf>,
    #[arg(
        long,
        value_name = "FILE",
        help = "Write the readings as a baseline to hold later runs to"
    )]
    pub record: Option<PathBuf>,
    #[arg(
        long,
        value_name = "FILE",
        help = "Fail naming each reading worse than this baseline by more than its tolerance"
    )]
    pub against: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct Timing {
    #[command(flatten)]
    pub chosen: Chosen,
}

#[derive(Debug, Args)]
pub struct Candidates {
    #[arg(value_name = "FILE")]
    pub file: PathBuf,
    #[arg(long, help = "Leave the throughput columns empty and run no timing")]
    pub no_timing: bool,
}
