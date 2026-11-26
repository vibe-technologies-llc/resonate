mod analysis;
mod cost;
mod error;
mod grammar;
mod lab;
mod report;
mod signal;
mod subjects;
mod suites;

use std::{fs, path::Path, process::ExitCode};

use clap::Parser;

use crate::{
    error::{Error, Result},
    grammar::{Chosen, Command, Grammar, Measuring},
    lab::{
        readings::{self, Measured},
        runner::{measured_everywhere, timed},
        subject::Subject,
        suite::Suite,
    },
    suites::resample::{PAIRS, Resample, tuning},
};

fn chosen(named: &[String]) -> Result<Vec<&'static dyn Suite>> {
    if named.is_empty() {
        return Ok(suites::all());
    }
    named
        .iter()
        .map(|name| {
            suites::named(name).ok_or_else(|| Error::UnknownSuite {
                named: name.clone(),
            })
        })
        .collect()
}

fn subjects_of(suite: &dyn Suite, only: Option<&str>) -> Result<Vec<Box<dyn Subject>>> {
    Ok(suite
        .subjects()?
        .into_iter()
        .filter(|subject| only.is_none_or(|words| subject.name().0.contains(words)))
        .collect())
}

fn written(path: &Path, text: &str) -> Result<()> {
    fs::write(path, text).map_err(|source| Error::Write {
        path: path.to_owned(),
        source,
    })
}

fn listed() -> Result<()> {
    for suite in suites::all() {
        println!("{}: {}", suite.name(), suite.about());

        let pairs: Vec<String> = suite.pairs().iter().map(ToString::to_string).collect();

        println!("  at {}", pairs.join(", "));
        for subject in suite.subjects()? {
            println!("  {}", subject.name());
        }
    }
    Ok(())
}

fn measured(measuring: &Measuring) -> Result<()> {
    let Chosen { suites, only } = &measuring.chosen;
    let mut everything: Vec<Measured> = Vec::new();
    let mut ran: Vec<&str> = Vec::new();

    if let Some(directory) = &measuring.plots {
        fs::create_dir_all(directory).map_err(|source| Error::Write {
            path: directory.clone(),
            source,
        })?;
    }
    for suite in chosen(suites)? {
        let subjects = subjects_of(suite, only.as_deref())?;
        let measured = measured_everywhere(suite, &subjects)?;

        println!("## {}\n", suite.title());
        print!("{}", suite.report(&subjects, &measured));
        if let Some(directory) = &measuring.plots {
            suite.plotted(directory, &measured)?;
        }
        ran.push(suite.name());
        everything.extend(measured);
    }

    let lines = readings::written(&everything);

    if let Some(path) = &measuring.tsv {
        written(path, &lines)?;
    }
    if let Some(path) = &measuring.record {
        written(path, &lines)?;
    }
    if let Some(path) = &measuring.against {
        held_to(path, &everything, &ran, only.as_deref())?;
    }
    Ok(())
}

fn held_to(path: &Path, measured: &[Measured], ran: &[&str], only: Option<&str>) -> Result<()> {
    let text = fs::read_to_string(path).map_err(|source| Error::Read {
        path: path.to_owned(),
        source,
    })?;
    let kept = readings::read(&text).map_err(|unreadable| Error::UnreadableBaseline {
        file: path.to_owned(),
        line: unreadable.line,
    })?;
    let kept = readings::in_scope(kept, ran, only);
    let weighed = readings::weighed(measured, &kept);

    for gone in &weighed.gone {
        eprintln!(
            "no longer measured: {} {} at {}: {}",
            gone.suite, gone.subject, gone.pair, gone.key
        );
    }
    for regression in &weighed.regressions {
        eprintln!(
            "worse: {} {} at {}: {} was {} and is {}",
            regression.case.suite,
            regression.case.subject,
            regression.case.pair,
            regression.metric.key,
            regression.then,
            regression.now
        );
    }
    eprintln!(
        "{} readings held to {}, {} worse",
        weighed.compared,
        path.display(),
        weighed.regressions.len()
    );
    if !weighed.regressions.is_empty() {
        return Err(Error::Regressed {
            count: weighed.regressions.len(),
        });
    }
    if !weighed.gone.is_empty() {
        return Err(Error::Unmeasured {
            count: weighed.gone.len(),
        });
    }
    if weighed.compared == 0 {
        return Err(Error::NothingHeld {
            file: path.to_owned(),
        });
    }
    Ok(())
}

fn timed_suites(chosen_ones: &Chosen) -> Result<()> {
    for suite in chosen(&chosen_ones.suites)? {
        let subjects = subjects_of(suite, chosen_ones.only.as_deref())?;
        let costs = timed(suite, &subjects)?;

        println!("## {}\n", suite.title());
        print!("{}", suite.costs(&subjects, &costs));
    }
    Ok(())
}

fn weighed_candidates(file: &Path, timing: bool) -> Result<()> {
    let subjects = tuning::candidates(file)?;
    let costs = if timing {
        timed(&Resample, &subjects)?
    } else {
        Vec::new()
    };
    let measured = measured_everywhere(&Resample, &subjects)?;

    print!("{}", tuning::verdicts(&PAIRS, &subjects, &measured, &costs));
    Ok(())
}

fn run() -> Result<()> {
    match Grammar::parse().command {
        Command::List => listed(),
        Command::Measure(measuring) => measured(&measuring),
        Command::Time(timing) => timed_suites(&timing.chosen),
        Command::Candidates(candidates) => {
            weighed_candidates(&candidates.file, !candidates.no_timing)
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");

            let mut cause = std::error::Error::source(&error);

            while let Some(underneath) = cause {
                eprintln!("  because {underneath}");
                cause = underneath.source();
            }
            ExitCode::FAILURE
        }
    }
}
