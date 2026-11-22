use std::{
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};

use crate::{
    cost::{self, Cost},
    error::Result,
    lab::{
        pair::Pair,
        readings::Measured,
        subject::{Subject, SubjectName},
        suite::Suite,
    },
};

pub struct Timed {
    pub subject: SubjectName,
    pub pair: Pair,
    pub cost: Cost,
}

fn jobs<'a>(
    subjects: &'a [Box<dyn Subject>],
    pairs: &[Pair],
) -> Vec<(usize, Pair, &'a dyn Subject)> {
    pairs
        .iter()
        .flat_map(|&pair| {
            subjects
                .iter()
                .enumerate()
                .filter(move |(_, subject)| subject.supports(pair))
                .map(move |(index, subject)| (index, pair, subject.as_ref()))
        })
        .collect()
}

pub fn measured_everywhere(
    suite: &dyn Suite,
    subjects: &[Box<dyn Subject>],
) -> Result<Vec<Measured>> {
    let pairs = suite.pairs();
    let jobs = jobs(subjects, &pairs);
    let next = AtomicUsize::new(0);
    let workers = thread::available_parallelism().map_or(1, usize::from);

    let finished: Vec<Result<Vec<(usize, Measured)>>> = thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();

                    while let Some(&(index, pair, subject)) =
                        jobs.get(next.fetch_add(1, Ordering::Relaxed))
                    {
                        let outcome = suite.measure(subject, pair)?;

                        eprintln!("measured {} at {pair}", subject.name());
                        done.push((
                            index,
                            Measured {
                                suite: suite.name(),
                                subject: subject.name(),
                                family: subject.family(),
                                pair,
                                readings: outcome.readings,
                                curves: outcome.curves,
                            },
                        ));
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
    all.sort_by_key(|(index, measured)| {
        (pairs.iter().position(|pair| *pair == measured.pair), *index)
    });
    Ok(all.into_iter().map(|(_, measured)| measured).collect())
}

pub fn timed(suite: &dyn Suite, subjects: &[Box<dyn Subject>]) -> Result<Vec<Timed>> {
    let mut costs = Vec::new();

    for pair in suite.timed_pairs() {
        for subject in subjects.iter().filter(|subject| subject.supports(pair)) {
            let layout = subject.layout_or(suite.timed_layout());
            let spent = cost::cost(subject.as_ref(), pair, layout)?;

            eprintln!(
                "timed {} at {pair}: {:.3} % of a core",
                subject.name(),
                spent.share_of_a_core
            );
            costs.push(Timed {
                subject: subject.name(),
                pair,
                cost: spent,
            });
        }
    }
    Ok(costs)
}

pub fn cost_of(costs: &[Timed], subject: &SubjectName, pair: Pair) -> Option<Cost> {
    costs
        .iter()
        .find(|timed| &timed.subject == subject && timed.pair == pair)
        .map(|timed| timed.cost)
}
