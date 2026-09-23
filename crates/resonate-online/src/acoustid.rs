use std::{cmp::Reverse, sync::Arc, time::Duration};

use resonate_analysis::print_clip;
use resonate_core::{Chromaprint, Mbid, SourceId};
use resonate_library::{Credit, Fingerprints, LookupOp, Printed, RecordingMatch, Sounded};
use resonate_listen::{Clip, Heard as HeardClip, Recogniser};
use serde::Deserialize;

use crate::{Client, Host, query::Params};

const ACOUSTID: &str = "acoustid";
const ANSWERED: &str = "ok";
const WHAT_IS_ASKED_FOR: &str = "recordings";
const WHOLE_SCORE: f64 = 100.0;
const SHORTEST_LENGTH: u64 = 1;
const A_CLIP_HEARD_AT_LEAST: u8 = 50;

#[derive(Deserialize)]
struct Answer {
    status: String,
    #[serde(default)]
    results: Vec<Found>,
}

#[derive(Deserialize)]
struct Found {
    score: f64,
    #[serde(default)]
    recordings: Vec<Heard>,
}

#[derive(Deserialize)]
struct Heard {
    id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    artists: Vec<Credited>,
}

#[derive(Deserialize)]
struct Credited {
    #[serde(default)]
    id: Option<String>,
    name: String,
    #[serde(default)]
    joinphrase: Option<String>,
}

pub struct AcoustId {
    client: Arc<Client>,
    key: String,
    source: SourceId,
}

impl AcoustId {
    pub fn new(client: Arc<Client>, key: String) -> Self {
        Self {
            client,
            key,
            source: SourceId::new(ACOUSTID).unwrap_or_else(|_| SourceId::local()),
        }
    }
}

impl Fingerprints for AcoustId {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn recognise(&self, sounded: &Sounded) -> resonate_library::Result<Printed> {
        let length = sounded.length.unwrap_or_else(|| sounded.print.length());
        Ok(self.looked_up(&sounded.print, length)?)
    }
}

impl AcoustId {
    fn looked_up(&self, print: &Chromaprint, length: Duration) -> crate::Result<Printed> {
        let query = Params::new()
            .with("client", &self.key)
            .with("meta", WHAT_IS_ASKED_FOR)
            .with(
                "duration",
                &length.as_secs().max(SHORTEST_LENGTH).to_string(),
            )
            .with("fingerprint", print.encoded())
            .finish();
        let answer: Option<Answer> = self.client.json(
            Host::AcoustId,
            LookupOp::Recognise,
            &format!("/lookup{query}"),
        )?;
        Ok(printed(answer))
    }
}

impl Recogniser for AcoustId {
    fn service(&self) -> &SourceId {
        &self.source
    }

    fn recognise(&self, clip: &Clip) -> resonate_listen::Result<Option<HeardClip>> {
        let Some(print) = print_clip(&clip.samples, clip.rate.hz(), usize::from(clip.channels))
        else {
            return Ok(None);
        };
        let printed = self
            .looked_up(&print, clip.length())
            .map_err(|error| error.into_listen_error(self.source.clone()))?;
        Ok(heard_clip(printed, &self.source))
    }
}

fn heard_clip(printed: Printed, service: &SourceId) -> Option<HeardClip> {
    let Printed::Recognised(matches) = printed else {
        return None;
    };
    let best = matches
        .into_iter()
        .find(|found| found.score >= A_CLIP_HEARD_AT_LEAST)?;
    Some(HeardClip {
        artist: Some(best.credited_as()).filter(|artist| !artist.is_empty()),
        title: best.title,
        album: None,
        year: None,
        isrc: best.isrcs.first().cloned(),
        recording: Some(best.recording),
        picture: None,
        link: None,
        by: service.clone(),
    })
}

fn printed(answer: Option<Answer>) -> Printed {
    let Some(answer) = answer.filter(|answer| answer.status == ANSWERED) else {
        return Printed::Nothing;
    };

    let mut matches: Vec<RecordingMatch> = Vec::new();
    for found in answer.results {
        let score = (found.score * WHOLE_SCORE).round().clamp(0.0, WHOLE_SCORE) as u8;
        for heard in found.recordings {
            let Some(named) = heard_as(heard, score) else {
                continue;
            };
            match matches
                .iter_mut()
                .find(|held| held.recording == named.recording)
            {
                Some(held) if held.score >= named.score => {}
                Some(held) => *held = named,
                None => matches.push(named),
            }
        }
    }
    matches.sort_by_key(|found| Reverse(found.score));

    if matches.is_empty() {
        Printed::Nothing
    } else {
        Printed::Recognised(matches)
    }
}

fn heard_as(heard: Heard, score: u8) -> Option<RecordingMatch> {
    let title = heard.title.filter(|title| !title.trim().is_empty())?;
    Some(RecordingMatch {
        recording: Mbid::new(&heard.id).ok()?,
        score,
        title,
        credit: heard
            .artists
            .into_iter()
            .map(|artist| Credit {
                name: artist.name,
                joined_by: artist.joinphrase.unwrap_or_default(),
                mbid: artist.id.as_deref().and_then(|id| Mbid::new(id).ok()),
            })
            .collect(),
        length: heard
            .duration
            .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok()),
        isrcs: Vec::new(),
        releases: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOOKUP: &str = include_str!("../tests/fixtures/acoustid_lookup.json");

    fn answered(text: &str) -> Printed {
        printed(Some(serde_json::from_str(text).expect("a lookup answer")))
    }

    #[test]
    fn a_lookup_answers_each_titled_recording_once_under_its_best_score() {
        let Printed::Recognised(found) = answered(LOOKUP) else {
            panic!("the lookup recognised nothing");
        };

        assert_eq!(found.len(), 2);
        let first = &found[0];
        assert_eq!(
            first.recording,
            Mbid::new("cd2e7c47-16f5-46c6-a37c-a1eb7bf599ff").expect("an mbid")
        );
        assert_eq!(first.score, 97);
        assert_eq!(first.title, "Lower Your Eyelids to Die With the Sun");
        assert_eq!(first.length, Some(Duration::from_secs(639)));
        assert_eq!(first.credit.len(), 1);
        assert_eq!(first.credit[0].name, "M83");
        assert!(first.credit[0].mbid.is_some());

        let second = &found[1];
        assert_eq!(second.score, 51);
        assert_eq!(second.title, "Echoes");
        assert_eq!(second.credit[0].joined_by, " & ");
        assert_eq!(second.credit[1].name, "Friends");
        assert_eq!(second.credit[1].mbid, None);
        assert_eq!(second.length, Some(Duration::from_millis(212_400)));
    }

    #[test]
    fn a_clip_is_named_by_its_best_match_above_half_a_score() {
        let source = SourceId::new(ACOUSTID).expect("a service name");
        let heard = heard_clip(answered(LOOKUP), &source).expect("a match above half");
        assert_eq!(heard.title, "Lower Your Eyelids to Die With the Sun");
        assert_eq!(heard.artist.as_deref(), Some("M83"));
        assert!(heard.recording.is_some());
        assert_eq!(heard.by, source);
        assert_eq!(heard_clip(Printed::Nothing, &source), None);
    }

    #[test]
    fn an_answer_that_is_not_ok_or_holds_nothing_recognises_nothing() {
        assert_eq!(
            answered(r#"{"status": "error", "error": {"code": 4, "message": "invalid API key"}}"#),
            Printed::Nothing
        );
        assert_eq!(
            answered(r#"{"status": "ok", "results": []}"#),
            Printed::Nothing
        );
        assert_eq!(printed(None), Printed::Nothing);
    }
}
