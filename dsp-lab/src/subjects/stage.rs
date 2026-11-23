use resonate_core::{ChannelLayout, SampleFormat, StreamSpec};
use resonate_dsp::Processor;

use crate::{
    error::{Error, Result},
    lab::{
        pair::Pair,
        subject::{Family, Running, Subject, SubjectName},
    },
};

type Build = dyn Fn(Pair, ChannelLayout) -> std::result::Result<Box<dyn Processor>, resonate_dsp::Error>
    + Send
    + Sync;

pub struct Stage {
    name: String,
    layout: Option<ChannelLayout>,
    build: Box<Build>,
}

impl Stage {
    pub fn new(
        name: impl Into<String>,
        build: impl Fn(
            Pair,
            ChannelLayout,
        ) -> std::result::Result<Box<dyn Processor>, resonate_dsp::Error>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            layout: None,
            build: Box::new(build),
        }
    }

    pub const fn taking(mut self, layout: ChannelLayout) -> Self {
        self.layout = Some(layout);
        self
    }
}

impl Subject for Stage {
    fn name(&self) -> SubjectName {
        SubjectName(self.name.clone())
    }

    fn family(&self) -> Family {
        Family::Ours
    }

    fn layout(&self) -> Option<ChannelLayout> {
        self.layout
    }

    fn open(&self, pair: Pair, layout: ChannelLayout, block: usize) -> Result<Box<dyn Running>> {
        let refused = |source| Error::Ours {
            subject: self.name(),
            pair,
            source,
        };
        let processor = (self.build)(pair, layout).map_err(refused)?;

        Driven::open(processor, pair.from, layout, block)
            .map(|driven| Box::new(driven) as Box<dyn Running>)
            .map_err(refused)
    }
}

pub struct Driven {
    processor: Box<dyn Processor>,
    channels_in: usize,
    channels_out: usize,
    most_in: usize,
    scratch: Vec<f64>,
}

impl Driven {
    pub fn open(
        mut processor: Box<dyn Processor>,
        rate: resonate_core::SampleRate,
        layout: ChannelLayout,
        block: usize,
    ) -> std::result::Result<Self, resonate_dsp::Error> {
        let spec = StreamSpec::new(rate, layout, SampleFormat::F32);
        let channels_in = usize::from(layout.count().get());
        let channels_out = usize::from(processor.output_spec(spec).channel_count().get());

        let most_out = processor.prepare(spec, block)?;
        let room = most_out.max(processor.max_flush_frames()) * channels_out;

        Ok(Self {
            processor,
            channels_in,
            channels_out,
            most_in: block,
            scratch: vec![0.0; room],
        })
    }
}

impl Running for Driven {
    fn push(&mut self, input: &[f64], output: &mut Vec<f64>) -> Result<()> {
        for block in input.chunks(self.most_in * self.channels_in) {
            let mut rest = block;
            while !rest.is_empty() {
                let count = self.processor.process(rest, &mut self.scratch);

                output.extend_from_slice(&self.scratch[..count.frames_out * self.channels_out]);
                if count.frames_in == 0 && count.frames_out == 0 {
                    break;
                }
                rest = &rest[count.frames_in * self.channels_in..];
            }
        }
        Ok(())
    }

    fn finish(&mut self, output: &mut Vec<f64>) -> Result<()> {
        let flushed = self.processor.flush(&mut self.scratch);

        output.extend_from_slice(&self.scratch[..flushed * self.channels_out]);
        Ok(())
    }

    fn channels_out(&self) -> usize {
        self.channels_out
    }

    fn starts_over(&mut self) -> bool {
        self.processor.reset();
        true
    }

    fn claimed_latency_frames(&self) -> Option<f64> {
        Some(self.processor.latency_frames())
    }
}
