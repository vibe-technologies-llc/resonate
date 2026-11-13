use std::time::{SystemTime, UNIX_EPOCH};

use resonate_engine::{Learning, Learnt, NodeName, Plays, Trial, TrialVerdict, Unplayed};
use rusqlite::params;

use crate::{
    Error, MobilePlayer, Result, StoreOp,
    catalog::Catalog,
    rendering::{Kinds, Rendering},
    words,
};

const SPELLINGS_APART: char = ' ';

const NAMES_APART: &str = ", ";

const KEPT: &str = "SELECT node, name, plays FROM renderers_learnt ORDER BY name, node";

const LEARNT: &str = "INSERT INTO renderers_learnt (node, name, plays, learnt)
     VALUES (?1, ?2, ?3, ?4)
     ON CONFLICT (node) DO UPDATE
        SET name = excluded.name, plays = excluded.plays, learnt = excluded.learnt";

const FORGOTTEN: &str = "DELETE FROM renderers_learnt WHERE node = ?1";

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TrialHeard {
    pub played: bool,
    pub said: String,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RendererLearnt {
    pub node: String,
    pub name: String,
    pub said: String,
}

struct Kept {
    node: NodeName,
    name: String,
    plays: Plays,
}

impl Catalog {
    fn renderers_learnt(&self) -> Result<Vec<Kept>> {
        let connection = self.0.lock();
        let mut statement = connection
            .prepare(KEPT)
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|source| Error::store(StoreOp::Read, source))?;
        rows.map(|row| {
            row.map(|(node, name, plays)| Kept {
                node: NodeName::new(node),
                name,
                plays: plays_read(&plays),
            })
            .map_err(|source| Error::store(StoreOp::Read, source))
        })
        .collect()
    }

    fn keep_what_was_learnt(&self, node: &NodeName, name: &str, plays: &Plays) -> Result<()> {
        self.0
            .lock()
            .execute(
                LEARNT,
                params![node.as_str(), name, plays_written(plays), seconds_now()],
            )
            .map(drop)
            .map_err(|source| Error::store(StoreOp::Write, source))
    }

    fn forget_what_was_learnt(&self, node: &str) -> Result<()> {
        self.0
            .lock()
            .execute(FORGOTTEN, params![node])
            .map(drop)
            .map_err(|source| Error::store(StoreOp::Write, source))
    }
}

fn plays_written(plays: &Plays) -> String {
    plays
        .trials()
        .map(Trial::spelling)
        .collect::<Vec<_>>()
        .join(&SPELLINGS_APART.to_string())
}

fn plays_read(written: &str) -> Plays {
    written
        .split(SPELLINGS_APART)
        .filter(|spelling| !spelling.is_empty())
        .filter_map(|spelling| {
            let trial = Trial::spelled(spelling);
            if trial.is_none() {
                tracing::warn!(spelling, "a trial kept by a later build was passed over");
            }
            trial
        })
        .collect()
}

fn seconds_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
        })
}

fn learnt_of(kept: &[Kept]) -> Learnt {
    kept.iter()
        .map(|kept| (kept.node.clone(), kept.plays.clone()))
        .collect()
}

fn trial_named(trial: Trial) -> &'static str {
    match trial {
        Trial::FlacAt192kHz => words::renderers::flac_at_192(),
        Trial::FlacAt176kHz => words::renderers::flac_at_176(),
        Trial::FlacAt384kHz => words::renderers::flac_at_384(),
        Trial::FlacAt352kHz => words::renderers::flac_at_352(),
        Trial::SixChannels => words::renderers::six_channels(),
        Trial::EightChannels => words::renderers::eight_channels(),
        Trial::SixChannelsAt96kHz => words::renderers::six_channels_at_96(),
        Trial::EightChannelsAt96kHz => words::renderers::eight_channels_at_96(),
    }
}

fn verdict_named(verdict: TrialVerdict) -> &'static str {
    match verdict {
        TrialVerdict::Played => words::renderers::played(),
        TrialVerdict::Untried => words::renderers::untried(),
        TrialVerdict::Unplayed(Unplayed::Refused) => words::renderers::refused(),
        TrialVerdict::Unplayed(Unplayed::NeverFetched) => words::renderers::never_fetched(),
        TrialVerdict::Unplayed(Unplayed::StoppedFetching) => words::renderers::stopped_fetching(),
        TrialVerdict::Unplayed(Unplayed::Errored) => words::renderers::errored(),
        TrialVerdict::Unplayed(Unplayed::NotPlaying) => words::renderers::not_playing(),
        TrialVerdict::Unplayed(Unplayed::ClockStill) => words::renderers::clock_still(),
    }
}

fn plays_said(plays: &Plays) -> String {
    if plays.is_empty() {
        return words::renderers::played_none_of_them().to_owned();
    }
    words::renderers::plays(
        plays
            .trials()
            .map(trial_named)
            .collect::<Vec<_>>()
            .join(NAMES_APART),
    )
}

fn heard_of(learning: &Learning) -> Vec<TrialHeard> {
    learning
        .verdicts()
        .iter()
        .map(|(trial, verdict)| TrialHeard {
            played: *verdict == TrialVerdict::Played,
            said: words::renderers::heard(trial_named(*trial), verdict_named(*verdict)),
        })
        .collect()
}

fn renderer_unlearnt(source: resonate_upnp::Error) -> Error {
    match source {
        resonate_upnp::Error::InUse(_) => Error::RendererInUse,
        source => Error::RendererUnlearnt {
            source: Box::new(source),
        },
    }
}

fn receiver_unlearnt(source: resonate_cast::Error) -> Error {
    match source {
        resonate_cast::Error::InUse(_) => Error::RendererInUse,
        source => Error::ReceiverUnlearnt {
            source: Box::new(source),
        },
    }
}

impl Kinds {
    fn learn(&self, node: &NodeName) -> Result<Learning> {
        if let Some(sink) = self
            .upnp
            .sinks()
            .into_iter()
            .find(|sink| sink.is_named(node))
        {
            return self.upnp.learn(sink.id).map_err(renderer_unlearnt);
        }
        let sink = self
            .cast
            .sinks()
            .into_iter()
            .find(|sink| sink.is_named(node))
            .ok_or(Error::UnknownRenderer)?;
        self.cast.learn(sink.id).map_err(receiver_unlearnt)
    }

    fn teach(&self, learnt: &Learnt) {
        self.upnp.teach(learnt.clone());
        self.cast.teach(learnt.clone());
    }
}

impl Rendering {
    pub(crate) fn taught_from(&self, catalog: &Catalog) -> Result<()> {
        if let Some(kinds) = self.kinds() {
            kinds.teach(&learnt_of(&catalog.renderers_learnt()?));
        }
        Ok(())
    }
}

#[uniffi::export]
impl MobilePlayer {
    pub fn learn_what_renderer_plays(&self, node: String) -> Result<Vec<TrialHeard>> {
        let kinds = self.rendering.kinds().ok_or(Error::RenderersUnreachable)?;
        let listed = self.rendering.named(&node).ok_or(Error::UnknownRenderer)?;
        let node = NodeName::new(listed.node.as_str());
        let learning = kinds.learn(&node)?;
        self.catalog
            .keep_what_was_learnt(&node, &listed.name, &learning.plays())?;
        self.rendering.taught_from(&self.catalog)?;
        Ok(heard_of(&learning))
    }

    pub fn renderers_learnt(&self) -> Result<Vec<RendererLearnt>> {
        Ok(self
            .catalog
            .renderers_learnt()?
            .into_iter()
            .map(|kept| RendererLearnt {
                said: plays_said(&kept.plays),
                node: kept.node.as_str().to_owned(),
                name: kept.name,
            })
            .collect())
    }

    pub fn forget_what_renderer_plays(&self, node: String) -> Result<()> {
        self.catalog.forget_what_was_learnt(&node)?;
        self.rendering.taught_from(&self.catalog)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_renderer_plays_is_kept_in_spellings_and_one_this_build_does_not_know_is_passed_over()
    {
        let plays = Plays::from_iter([Trial::FlacAt192kHz, Trial::SixChannelsAt96kHz]);

        let written = plays_written(&plays);

        assert_eq!(written, "flac-192000 channels-6-96000");
        assert_eq!(plays_read(&written), plays);
        assert_eq!(
            plays_read("flac-768000 channels-6"),
            Plays::from_iter([Trial::SixChannels])
        );
        assert_eq!(plays_read(""), Plays::default());
    }

    #[test]
    fn what_a_renderer_plays_is_said_in_a_sentence_and_none_is_said_too() {
        assert_eq!(
            plays_said(&Plays::from_iter([Trial::FlacAt192kHz, Trial::SixChannels])),
            "Plays FLAC at 192 kHz, 5.1 at 48 kHz"
        );
        assert_eq!(plays_said(&Plays::default()), "Played none of them");
    }
}
