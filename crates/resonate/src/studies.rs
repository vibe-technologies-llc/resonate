use resonate_library::{Agreement, Library, StudiedTrack, StudyFilter, Verdict};

use crate::{Result, table::Table};

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
    println!(
        "studied {} | genuine {} | suspect {} | fake {} | lossy {} | not judged {} | recognised \
         {recognised} | misnamed {misnamed}",
        every.len(),
        counted(Verdict::Genuine),
        counted(Verdict::Suspect),
        counted(Verdict::Fake),
        counted(Verdict::Lossy),
        counted(Verdict::NotJudged),
    );

    let shown = if filter == StudyFilter::default() {
        every
    } else {
        library.studies(filter)?
    };
    if shown.is_empty() {
        println!("nothing studied matches");
        return Ok(());
    }

    let mut table = Table::new(vec![
        "VERDICT", "CUTOFF", "BITS", "LOUDNESS", "RANGE", "HEARD AS", "ARTIST", "TITLE",
    ]);
    for track in &shown {
        table.push(row(track));
    }
    print!("{}", table.render());
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
        (None, Some(Agreement::Unheard)) => "not recognised".to_owned(),
        (None, _) => NOTHING.to_owned(),
    };

    vec![
        studied.verdict.as_str().to_owned(),
        studied.cutoff.map_or_else(
            || NOTHING.to_owned(),
            |cutoff| format!("{:.1} kHz", cutoff.hz as f32 / HZ_A_KILOHERTZ),
        ),
        match (studied.bits_in_use, studied.declared_bits) {
            (Some(used), Some(declared)) if used < declared => format!("{used}/{declared}"),
            (_, Some(declared)) => declared.to_string(),
            (Some(used), None) => used.to_string(),
            (None, None) => NOTHING.to_owned(),
        },
        studied
            .loudness
            .map_or_else(|| NOTHING.to_owned(), |lufs| format!("{lufs:.1} LUFS")),
        studied
            .loudness_range
            .map_or_else(|| NOTHING.to_owned(), |lu| format!("{lu:.1} LU")),
        heard,
        track.artist.clone().unwrap_or_else(|| NOTHING.to_owned()),
        track.title.clone(),
    ]
}
