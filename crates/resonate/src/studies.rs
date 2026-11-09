use resonate_core::{FrameSpan, MediaLocation};
use resonate_library::{Agreement, Library, StudiedTrack, StudyFilter, Verdict};

use crate::{
    Error, Result,
    table::{self, Table},
    words,
};

const NOTHING: &str = "-";
const HZ_A_KILOHERTZ: f32 = 1_000.0;

pub fn print(library: &Library, filter: StudyFilter) -> Result<()> {
    let every = library.studies(StudyFilter::default())?;
    let counted = |verdict: Verdict| {
        every
            .iter()
            .filter(|track| track.studied.verdict == verdict)
            .count()
    };
    let misnamed = every
        .iter()
        .filter(|track| track.studied.agreement == Some(Agreement::Disagrees))
        .count();
    let recognised = every
        .iter()
        .filter(|track| track.studied.recognised.is_some())
        .count();
    said!(
        "{} | {}",
        words::studies::judged(
            counted(Verdict::Fake),
            counted(Verdict::Genuine),
            counted(Verdict::Lossy),
            every.len(),
            counted(Verdict::Suspect),
        ),
        words::studies::named(misnamed, recognised, counted(Verdict::NotJudged)),
    );

    let shown = if filter == StudyFilter::default() {
        every
    } else {
        library.studies(filter)?
    };
    if shown.is_empty() {
        said!("{}", words::studies::none_match());
        return Ok(());
    }

    let mut table = Table::new(vec![
        words::studies::verdict(),
        words::studies::cutoff(),
        words::studies::bits(),
        words::studies::loudness(),
        words::studies::range(),
        words::studies::heard_as(),
        words::studies::artist(),
        words::studies::title(),
    ]);
    for track in &shown {
        table.push(row(track));
    }
    said_on!("{}", table.render());
    Ok(())
}

pub fn take(library: &Library, location: &MediaLocation, span: Option<FrameSpan>) -> Result<()> {
    let track = location
        .as_path()
        .and_then(|path| library.track_at(path, span).transpose())
        .transpose()?
        .ok_or_else(|| Error::NotInTheCatalog {
            location: location.clone(),
        })?;
    let taken = library
        .take_what_was_heard(track.id)?
        .ok_or_else(|| Error::NothingHeardAs {
            location: location.clone(),
        })?;

    said!(
        "{}",
        words::studies::taken(
            table::on_one_line(taken.artist.as_deref().unwrap_or(NOTHING)),
            taken.recording.to_string(),
            taken.score.to_string(),
            table::on_one_line(&taken.title),
        )
    );
    Ok(())
}

fn row(track: &StudiedTrack) -> Vec<String> {
    let studied = &track.studied;
    let heard = match (&studied.heard_as, studied.agreement) {
        (Some(heard), Some(Agreement::Disagrees)) => format!(
            "≠ {} — {}",
            heard.artist.as_deref().unwrap_or(NOTHING),
            heard.title
        ),
        (Some(heard), _) => format!("{} ({})", heard.title, heard.score),
        (None, Some(Agreement::Unheard)) => words::studies::not_recognised().to_owned(),
        (None, _) => NOTHING.to_owned(),
    };

    vec![
        studied.verdict.as_str().to_owned(),
        studied.cutoff.map_or_else(
            || NOTHING.to_owned(),
            |cutoff| words::studies::kilohertz(format!("{:.1}", cutoff.hz as f32 / HZ_A_KILOHERTZ)),
        ),
        match (studied.bits_in_use, studied.declared_bits) {
            (Some(used), Some(declared)) if used < declared => format!("{used}/{declared}"),
            (_, Some(declared)) => declared.to_string(),
            (Some(used), None) => used.to_string(),
            (None, None) => NOTHING.to_owned(),
        },
        studied.loudness.map_or_else(
            || NOTHING.to_owned(),
            |lufs| words::studies::lufs(format!("{lufs:.1}")),
        ),
        studied.loudness_range.map_or_else(
            || NOTHING.to_owned(),
            |lu| words::studies::lu(format!("{lu:.1}")),
        ),
        heard,
        track.artist.clone().unwrap_or_else(|| NOTHING.to_owned()),
        track.title.clone(),
    ]
}
