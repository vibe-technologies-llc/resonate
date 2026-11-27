#![no_main]

mod held;

use std::path::{Path, PathBuf};

use libfuzzer_sys::fuzz_target;
use resonate_codec::{
    CueNaming, SheetEdit, SheetField, SheetScope, SheetText, read_cue, read_cue_media, renamed_cue,
    the_best_a_cue_names, the_folder_a_cue_names, the_one_a_cue_names,
};
use resonate_core::{MediaLocation, SampleRate};

const NOWHERE: &str = "/nonexistent-resonate-fuzz";
const FIELDS: [SheetField; 10] = [
    SheetField::Title,
    SheetField::Performer,
    SheetField::Songwriter,
    SheetField::Isrc,
    SheetField::Catalog,
    SheetField::Date,
    SheetField::Genre,
    SheetField::Comment,
    SheetField::DiscNumber,
    SheetField::DiscTotal,
];
const VALUES: [Option<&str>; 4] = [None, Some("Slow Water"), Some("Ça Ira – Ω"), Some("")];

fn listed_beside(named: &str) -> Vec<String> {
    let alone = named.rsplit(['/', '\\']).next().unwrap_or(named);
    let stem = alone.rsplit_once('.').map_or(alone, |(stem, _)| stem);

    vec![
        alone.to_owned(),
        alone.to_uppercase(),
        format!("{stem}.flac"),
        format!("{stem}.WAV"),
        format!("{alone}.flac"),
    ]
}

fn edited_as_a_sheet(bytes: &[u8], tracks: u32) {
    let text = SheetText::read(bytes);
    let location = MediaLocation::local(PathBuf::from(NOWHERE));
    let scopes = [
        SheetScope::Disc,
        SheetScope::Track(1),
        SheetScope::Track(tracks.max(1)),
    ];

    for scope in scopes {
        for field in FIELDS {
            let _ = text.own(scope, field);
        }
    }

    let _ = text.names_the_track_once(tracks);

    for (at, field) in FIELDS.into_iter().enumerate() {
        let edits: Vec<SheetEdit> = scopes
            .into_iter()
            .map(|scope| SheetEdit {
                scope,
                field,
                value: VALUES[at % VALUES.len()].map(str::to_owned),
            })
            .collect();
        if let Ok(written) = text.edited(&location, &edits) {
            let _ = SheetText::read(&written);
            let _ = read_cue(&written);
        }
    }
}

fuzz_target!(|bytes: &[u8]| {
    let sheet = read_cue(bytes);
    let tracks = sheet
        .files
        .iter()
        .map(|file| file.tracks.len())
        .sum::<usize>();
    edited_as_a_sheet(bytes, u32::try_from(tracks).unwrap_or(u32::MAX));
    for file in &sheet.files {
        let _ = renamed_cue(bytes, &file.named, "renamed.flac");
        for index in 0..file.tracks.len() {
            for rate in [SampleRate::HZ_44100, SampleRate::HZ_384000] {
                let _ = file.span_of(index, rate, None);
            }
        }

        let listed = listed_beside(&file.named);
        let _ = the_one_a_cue_names(
            &file.named,
            listed
                .iter()
                .enumerate()
                .map(|(at, name)| (at, name.as_str())),
        );
        let _ = the_best_a_cue_names(
            listed
                .iter()
                .map(|name| (name, CueNaming::of(&file.named, name))),
        );
        let _ = the_folder_a_cue_names(Path::new(NOWHERE), &file.named);
    }

    let (sources, location) = held::held(bytes);
    let _ = read_cue_media(&sources, &location);
});
