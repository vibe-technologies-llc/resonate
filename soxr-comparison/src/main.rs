mod contender;
mod cost;
mod error;
mod fit;
mod measure;
mod plot;
mod report;
mod signal;
mod summary;

use std::{
    env,
    path::PathBuf,
    process::ExitCode,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

use resonate_core::SampleRate;

use crate::{
    contender::{CONTENDERS, Contender, Pair},
    cost::{Cost, cost},
    error::{Error, Result},
    summary::{Measured, measured},
};

const PAIRS: [Pair; 7] = [
    Pair::new(SampleRate::HZ_44100, SampleRate::HZ_48000),
    Pair::new(SampleRate::HZ_48000, SampleRate::HZ_44100),
    Pair::new(SampleRate::HZ_96000, SampleRate::HZ_48000),
    Pair::new(SampleRate::HZ_96000, SampleRate::HZ_44100),
    Pair::new(SampleRate::HZ_192000, SampleRate::HZ_48000),
    Pair::new(SampleRate::HZ_44100, SampleRate::HZ_96000),
    Pair::new(SampleRate::HZ_48000, SampleRate::HZ_96000),
];

const BUILDS_SHOWN: [Pair; 2] = [
    Pair::new(SampleRate::HZ_44100, SampleRate::HZ_48000),
    Pair::new(SampleRate::HZ_192000, SampleRate::HZ_48000),
];

struct Arguments {
    plots: Option<PathBuf>,
    timed: bool,
    measured: bool,
}

impl Arguments {
    fn parsed() -> Result<Self> {
        let mut arguments = Self {
            plots: None,
            timed: true,
            measured: true,
        };
        let mut given = env::args_os().skip(1);
        while let Some(argument) = given.next() {
            match argument.to_str() {
                Some("--plots") => {
                    arguments.plots = Some(
                        given
                            .next()
                            .map(PathBuf::from)
                            .ok_or(Error::MissingValue { flag: "--plots" })?,
                    );
                }
                Some("--no-timing") => arguments.timed = false,
                Some("--timing-only") => arguments.measured = false,
                _ => return Err(Error::UnknownArgument(argument)),
            }
        }
        Ok(arguments)
    }
}

fn timed() -> Result<Vec<(Contender, Pair, Cost)>> {
    let mut costs = Vec::new();
    for pair in PAIRS {
        for contender in CONTENDERS {
            let spent = cost(contender, pair)?;
            eprintln!(
                "timed {contender} at {pair}: {:.3} % of a core",
                spent.share_of_a_core
            );
            costs.push((contender, pair, spent));
        }
    }
    Ok(costs)
}

fn measured_everywhere() -> Result<Vec<Measured>> {
    let jobs: Vec<(Pair, Contender)> = PAIRS
        .iter()
        .flat_map(|&pair| CONTENDERS.iter().map(move |&contender| (pair, contender)))
        .collect();
    let next = AtomicUsize::new(0);
    let workers = thread::available_parallelism().map_or(1, usize::from);

    let finished: Vec<Result<Vec<Measured>>> = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    while let Some(&(pair, contender)) =
                        jobs.get(next.fetch_add(1, Ordering::Relaxed))
                    {
                        done.push(measured(contender, pair)?);
                        eprintln!("measured {contender} at {pair}");
                    }
                    Ok(done)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or_else(|_| Ok(Vec::new())))
            .collect()
    });

    let mut all = Vec::new();
    for done in finished {
        all.extend(done?);
    }
    Ok(all)
}

fn run() -> Result<()> {
    let arguments = Arguments::parsed()?;

    if arguments.timed {
        let costs = timed()?;
        print!("{}", report::costs(&PAIRS, &BUILDS_SHOWN, &costs));
    }

    if arguments.measured {
        let measured = measured_everywhere()?;
        for pair in PAIRS {
            print!("{}", report::quality(pair, &measured));
            if let Some(directory) = &arguments.plots {
                plot::plotted(directory, pair, &measured)?;
            }
        }
    }

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
