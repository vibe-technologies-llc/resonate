use resonate_core::{AppliedGain, ChannelLayout, SampleData, SampleFormat, StreamSpec};
use resonate_dsp::Chain;
use resonate_engine::{
    Decoded, EngineConfig, NodeName, OutputPlan, SinkFormats, SinkId, SinkInfo, plan_output,
};

use crate::{
    error::{Error, Result},
    lab::{
        pair::Pair,
        subject::{Family, Running, Subject, SubjectName},
    },
};

pub struct Played {
    name: String,
    source: Decoded,
    config: EngineConfig,
    plan: OutputPlan,
}

pub fn sink(formats: Vec<SinkFormats>) -> SinkInfo {
    let mut allowed_rates: Vec<_> = formats
        .iter()
        .flat_map(|entry| entry.rates.iter().copied())
        .collect();

    allowed_rates.sort_by_key(|rate| rate.hz());
    allowed_rates.dedup();
    SinkInfo {
        id: SinkId::new(1),
        name: NodeName::new("dsp-lab"),
        aliases: Vec::new(),
        description: "dsp-lab".to_owned(),
        is_default: true,
        is_hardware: true,
        port: None,
        profile: None,
        formats,
        allowed_rates,
        current_rate: None,
    }
}

impl Played {
    pub fn new(
        name: impl Into<String>,
        source: Decoded,
        sink: &SinkInfo,
        config: EngineConfig,
        gain: AppliedGain,
    ) -> Self {
        let plan = plan_output(source, sink, &config, gain);

        Self {
            name: name.into(),
            source,
            config,
            plan,
        }
    }

    pub const fn plan(&self) -> &OutputPlan {
        &self.plan
    }

    pub const fn source(&self) -> StreamSpec {
        self.source.spec
    }

    pub fn name(&self) -> SubjectName {
        SubjectName(self.name.clone())
    }

    pub fn pair(&self) -> Pair {
        Pair::new(self.source.spec.rate, self.plan.stream.rate)
    }

    pub fn stages(&self) -> String {
        let plan = &self.plan;
        let mut stages = Vec::new();

        if let Some((from, to)) = plan.remix {
            stages.push(format!("remix {from:?} to {to:?}"));
        }
        if let Some(restoring) = plan.restoration {
            stages.push(format!("restore {}", restoring.restoration.as_str()));
        }
        if plan.resample.is_some() {
            stages.push(format!(
                "resample {:?} {:?}",
                self.config.quality, self.config.filter_phase
            ));
        }
        if plan.convolution.is_some() {
            stages.push("convolve".to_owned());
        }
        if let Some(crossfeed) = plan.crossfeed {
            stages.push(format!("crossfeed {}", crossfeed.as_str()));
        }
        if plan.equalisation.is_some() {
            stages.push("equalise".to_owned());
        }
        if let Some(gain) = plan.gain {
            stages.push(format!("gain ×{:.3}", gain.amplitude()));
        }
        if plan.true_peak {
            stages.push("guard".to_owned());
        }
        if let Some(depth) = plan.dither_to {
            stages.push(format!(
                "dither {:?} {:?} to {depth}",
                self.config.dither, plan.shaping
            ));
        }
        if stages.is_empty() {
            stages.push("none".to_owned());
        }
        stages.join(", ")
    }
}

impl Subject for Played {
    fn name(&self) -> SubjectName {
        Self::name(self)
    }

    fn family(&self) -> Family {
        Family::Ours
    }

    fn supports(&self, pair: Pair) -> bool {
        pair == self.pair()
    }

    fn layout(&self) -> Option<ChannelLayout> {
        Some(self.source.spec.channels)
    }

    fn open(&self, pair: Pair, layout: ChannelLayout, block: usize) -> Result<Box<dyn Running>> {
        let refused = |source| Error::Ours {
            subject: self.name(),
            pair,
            source,
        };
        let source = StreamSpec {
            channels: layout,
            ..self.source.spec
        };

        let chain = self
            .plan
            .build_chain(source, &self.config, block)
            .map_err(refused)?;
        let channels_out = usize::from(self.plan.stream.channel_count().get());

        Ok(Box::new(Playing {
            room: vec![0.0; chain.max_output_frames().max(chain.max_flush_frames()) * channels_out],
            chain,
            subject: self.name(),
            pair,
            decoded_as: source.format,
            heard_as: self.plan.stream.format,
            channels_in: usize::from(layout.count().get()),
            channels_out,
            block,
        }))
    }
}

struct Playing {
    chain: Chain,
    subject: SubjectName,
    pair: Pair,
    decoded_as: SampleFormat,
    heard_as: SampleFormat,
    channels_in: usize,
    channels_out: usize,
    block: usize,
    room: Vec<f64>,
}

pub fn in_words(samples: &[f64], format: SampleFormat) -> Vec<f64> {
    let mut words = SampleData::silence(format, samples.len());
    let mut wide = vec![0.0; samples.len()];

    words.write_f64(samples);
    words.widen_into(0, &mut wide);
    wide
}

impl Playing {
    fn heard(&self, frames: usize, output: &mut Vec<f64>) {
        output.extend(in_words(
            &self.room[..frames * self.channels_out],
            self.heard_as,
        ));
    }
}

impl Running for Playing {
    fn push(&mut self, input: &[f64], output: &mut Vec<f64>) -> Result<()> {
        let decoded = in_words(input, self.decoded_as);

        for block in decoded.chunks(self.block * self.channels_in) {
            let mut rest = block;
            while !rest.is_empty() {
                let count = self.chain.process(rest, &mut self.room);

                self.heard(count.frames_out, output);
                if count.frames_in == 0 && count.frames_out == 0 {
                    break;
                }
                rest = &rest[count.frames_in * self.channels_in..];
            }
        }
        Ok(())
    }

    fn finish(&mut self, output: &mut Vec<f64>) -> Result<()> {
        let flushed = self
            .chain
            .flush(&mut self.room)
            .map_err(|source| Error::Ours {
                subject: self.subject.clone(),
                pair: self.pair,
                source,
            })?;

        self.heard(flushed, output);
        Ok(())
    }

    fn channels_out(&self) -> usize {
        self.channels_out
    }

    fn claimed_latency_frames(&self) -> Option<f64> {
        Some(self.chain.latency_frames())
    }
}
