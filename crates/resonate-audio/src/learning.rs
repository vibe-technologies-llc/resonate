use std::collections::{BTreeMap, BTreeSet};

use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};

use crate::NodeName;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Trial {
    FlacAt192kHz,
    FlacAt176kHz,
    SixChannels,
    EightChannels,
}

impl Trial {
    pub const ALL: [Self; 4] = [
        Self::FlacAt192kHz,
        Self::FlacAt176kHz,
        Self::SixChannels,
        Self::EightChannels,
    ];

    pub const fn spelling(self) -> &'static str {
        match self {
            Self::FlacAt192kHz => "flac-192000",
            Self::FlacAt176kHz => "flac-176400",
            Self::SixChannels => "channels-6",
            Self::EightChannels => "channels-8",
        }
    }

    pub fn spelled(spelling: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|trial| trial.spelling() == spelling.trim())
    }

    pub const fn rate(self) -> SampleRate {
        match self {
            Self::FlacAt192kHz => SampleRate::HZ_192000,
            Self::FlacAt176kHz => SampleRate::HZ_176400,
            Self::SixChannels | Self::EightChannels => SampleRate::HZ_48000,
        }
    }

    pub const fn layout(self) -> ChannelLayout {
        match self {
            Self::FlacAt192kHz | Self::FlacAt176kHz => ChannelLayout::Stereo,
            Self::SixChannels => ChannelLayout::Surround51,
            Self::EightChannels => ChannelLayout::Surround71,
        }
    }

    pub const fn is_a_rate(self) -> bool {
        matches!(self, Self::FlacAt192kHz | Self::FlacAt176kHz)
    }

    pub const fn spec(self, format: SampleFormat) -> StreamSpec {
        StreamSpec::new(self.rate(), self.layout(), format)
    }

    pub const fn wanting(self) -> Option<Self> {
        match self {
            Self::EightChannels => Some(Self::SixChannels),
            Self::FlacAt192kHz | Self::FlacAt176kHz | Self::SixChannels => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unplayed {
    Refused,
    NeverFetched,
    StoppedFetching,
    Errored,
    NotPlaying,
    ClockStill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TrialVerdict {
    Played,
    Unplayed(Unplayed),
    Untried,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Plays(BTreeSet<Trial>);

impl Plays {
    pub fn contains(&self, trial: Trial) -> bool {
        self.0.contains(&trial)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn trials(&self) -> impl Iterator<Item = Trial> + '_ {
        self.0.iter().copied()
    }

    pub fn rates(&self) -> impl Iterator<Item = SampleRate> + '_ {
        self.trials()
            .filter(|trial| trial.is_a_rate())
            .map(Trial::rate)
    }

    pub fn layouts(&self) -> impl Iterator<Item = ChannelLayout> + '_ {
        self.trials()
            .filter(|trial| !trial.is_a_rate())
            .map(Trial::layout)
    }
}

impl FromIterator<Trial> for Plays {
    fn from_iter<T: IntoIterator<Item = Trial>>(trials: T) -> Self {
        Self(trials.into_iter().collect())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Learnt(BTreeMap<NodeName, Plays>);

impl Learnt {
    pub fn of<'a>(&self, names: impl IntoIterator<Item = &'a NodeName>) -> Option<&Plays> {
        names.into_iter().find_map(|name| self.0.get(name))
    }

    pub fn insert(&mut self, device: NodeName, plays: Plays) {
        self.0.insert(device, plays);
    }

    pub fn remove(&mut self, device: &NodeName) -> Option<Plays> {
        self.0.remove(device)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&NodeName, &Plays)> {
        self.0.iter()
    }
}

impl FromIterator<(NodeName, Plays)> for Learnt {
    fn from_iter<T: IntoIterator<Item = (NodeName, Plays)>>(learnt: T) -> Self {
        Self(learnt.into_iter().collect())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Learning(Vec<(Trial, TrialVerdict)>);

impl Learning {
    pub fn heard(&mut self, trial: Trial, verdict: TrialVerdict) {
        self.0.push((trial, verdict));
    }

    pub fn verdict_on(&self, trial: Trial) -> Option<TrialVerdict> {
        self.0
            .iter()
            .find(|(tried, _)| *tried == trial)
            .map(|(_, verdict)| *verdict)
    }

    pub fn verdicts(&self) -> &[(Trial, TrialVerdict)] {
        &self.0
    }

    pub fn plays(&self) -> Plays {
        self.0
            .iter()
            .filter(|(_, verdict)| *verdict == TrialVerdict::Played)
            .map(|(trial, _)| *trial)
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unlearnt {
    NotListed,
    InUse,
    Unreached,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_trial_reads_back_from_its_spelling() {
        for trial in Trial::ALL {
            assert_eq!(Trial::spelled(trial.spelling()), Some(trial));
        }
        assert_eq!(Trial::spelled(" channels-6 "), Some(Trial::SixChannels));
        assert_eq!(Trial::spelled("flac-352800"), None);
    }

    #[test]
    fn a_learning_plays_only_what_played() {
        let mut learning = Learning::default();
        learning.heard(Trial::FlacAt192kHz, TrialVerdict::Played);
        learning.heard(
            Trial::FlacAt176kHz,
            TrialVerdict::Unplayed(Unplayed::ClockStill),
        );
        learning.heard(Trial::SixChannels, TrialVerdict::Played);
        learning.heard(Trial::EightChannels, TrialVerdict::Untried);

        let plays = learning.plays();

        assert_eq!(
            plays.trials().collect::<Vec<_>>(),
            [Trial::FlacAt192kHz, Trial::SixChannels]
        );
        assert_eq!(plays.rates().collect::<Vec<_>>(), [SampleRate::HZ_192000]);
        assert_eq!(
            plays.layouts().collect::<Vec<_>>(),
            [ChannelLayout::Surround51]
        );
        assert_eq!(
            learning.verdict_on(Trial::EightChannels),
            Some(TrialVerdict::Untried)
        );
    }

    #[test]
    fn what_was_learnt_is_found_under_any_name_a_device_answers_to() {
        let named = NodeName::new("cast:abc");
        let addressed = NodeName::new("cast:at-192-168-1-20-8009");
        let learnt: Learnt = [(addressed.clone(), Plays::from_iter([Trial::SixChannels]))]
            .into_iter()
            .collect();

        assert_eq!(
            learnt.of([&named, &addressed]),
            Some(&Plays::from_iter([Trial::SixChannels]))
        );
        assert_eq!(learnt.of([&named]), None);
    }
}
