use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use crate::{
    contender::{Channels, Contender, Pair, Stream},
    error::Result,
    signal::Noise,
};

const TIMED_SECONDS: u32 = 4;
const RUNS: usize = 5;
const REBUILDS: usize = 9;
const BLOCK: usize = 1_024;
const SEED: u64 = 0x0062_656E_6368;
const ROOM_FRAMES: usize = 65_536;

#[derive(Clone, Copy, Debug)]
pub struct Cost {
    pub share_of_a_core: f64,
    pub first_build: Duration,
    pub rebuilt: Duration,
}

pub fn cost(contender: Contender, pair: Pair) -> Result<Cost> {
    let started = Instant::now();
    let first = Stream::open(contender, pair, Channels::Stereo, BLOCK)?;
    let first_build = started.elapsed();
    drop(first);

    let mut rebuilt = Duration::MAX;
    for _ in 0..REBUILDS {
        let started = Instant::now();
        let stream = Stream::open(contender, pair, Channels::Stereo, BLOCK)?;
        rebuilt = rebuilt.min(started.elapsed());
        drop(black_box(stream));
    }

    let channels = Channels::Stereo.count();
    let frames = pair.from.hz() as usize * TIMED_SECONDS as usize;
    let input = Noise::seeded(SEED).samples(frames * channels);
    let mut output = Vec::with_capacity(ROOM_FRAMES * channels);
    let mut fastest = Duration::MAX;

    for _ in 0..RUNS {
        let mut stream = Stream::open(contender, pair, Channels::Stereo, BLOCK)?;
        let started = Instant::now();
        for block in input.chunks(BLOCK * channels) {
            output.clear();
            stream.push(black_box(block), &mut output)?;
            black_box(&output);
        }
        fastest = fastest.min(started.elapsed());
    }

    Ok(Cost {
        share_of_a_core: fastest.as_secs_f64() / f64::from(TIMED_SECONDS) * 100.0,
        first_build,
        rebuilt,
    })
}
