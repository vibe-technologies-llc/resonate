use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use resonate_core::{Isrc, Mbid};
use resonate_library::{
    ArtistPressings, Barcode, GroupAsked, RecordingAsked, Reference, ReleaseAsked, SongsAsked,
    Wording,
};
use resonate_musicbrainz::{
    AMENDED_FILE, Answer, Archives, Asked, Building, Consultation, Error, Keeping, Kept, Layer,
    Layered, Mirror, Mirrored, Packet, TABLES_FILE, built_from, held_for_building, packet_read,
    replicated, settled,
};

const RADIOHEAD: &str = "a74b1b7f-71a5-4011-9441-d0b5e4122711";
const THOM: &str = "8ed2e0b3-aa4c-4e13-bec3-dc7393421532";
const CREEP: &str = "6bde2be9-9a01-4b23-9a34-3a4ec2d2c6a0";
const ACOUSTIC: &str = "0a1ac1a2-6b8f-4e2a-9bc6-1f1c6f9d0001";
const HEARING: &str = "0a1ac1a2-6b8f-4e2a-9bc6-1f1c6f9d0002";
const PABLO: &str = "dd8f0c31-0cd6-3a6c-8e42-4ac1c69d5f84";
const PABLO_GROUP: &str = "dd8f0c31-0cd6-3a6c-8e42-4ac1c69d5f85";
const TRACK_ONE: &str = "1d1e1f10-0000-4000-8000-000000000001";
const TRACK_TWO: &str = "1d1e1f10-0000-4000-8000-000000000002";
const CREEP_CODE: &str = "GBAYE9200070";
const PABLO_BARCODE: &str = "724382775324";
const PROLIFIC: &str = "3e3e3e3e-0000-4000-8000-000000000001";
const ARTIST_PRESSINGS_PAGE: usize = 100;
const PROLIFIC_GROUPS: usize = 3;
const PRESSINGS_IN_A_PROLIFIC_GROUP: usize = 40;
const FIRST_PROLIFIC_ROW: usize = 100;

fn prolific_gid(kind: u8, at: usize) -> String {
    format!("{kind:x}f3e3e3e-0000-4000-8000-{at:012x}")
}

fn prolific_groups() -> Vec<String> {
    (0..PROLIFIC_GROUPS)
        .map(|group| {
            line(
                8,
                &[
                    (0, &(FIRST_PROLIFIC_ROW + group).to_string()),
                    (1, &prolific_gid(1, group)),
                    (2, &format!("Prolific {group}")),
                    (3, "3"),
                    (4, "1"),
                    (5, ""),
                ],
            )
        })
        .collect()
}

fn prolific_releases() -> Vec<String> {
    (0..PROLIFIC_GROUPS * PRESSINGS_IN_A_PROLIFIC_GROUP)
        .map(|at| {
            let group = at / PRESSINGS_IN_A_PROLIFIC_GROUP;
            line(
                14,
                &[
                    (0, &(FIRST_PROLIFIC_ROW + at).to_string()),
                    (1, &prolific_gid(2, at)),
                    (2, &format!("Prolific {group} pressing {at}")),
                    (3, "3"),
                    (4, &(FIRST_PROLIFIC_ROW + group).to_string()),
                    (5, "1"),
                    (10, ""),
                ],
            )
        })
        .collect()
}

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "resonate-musicbrainz-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("a scratch folder");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn line(width: usize, set: &[(usize, &str)]) -> String {
    let mut fields = vec!["\\N".to_owned(); width];
    for (at, value) in set {
        fields[*at] = (*value).to_owned();
    }
    fields.join("\t")
}

const DUMPED_AT_SEQUENCE: u64 = 100;
const SCHEMA: u32 = 31;

fn archived(into: impl Write, files: &[(String, String)]) {
    let packed = bzip2::write::BzEncoder::new(into, bzip2::Compression::fast());
    let mut builder = tar::Builder::new(packed);
    for (name, body) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, name, body.as_bytes())
            .expect("a file in the archive");
    }
    builder
        .into_inner()
        .expect("a whole archive")
        .finish()
        .expect("a finished archive")
        .flush()
        .expect("a flushed archive");
}

fn archive(path: &Path, tables: &[(&str, Vec<String>)]) {
    let file = fs::File::create(path).expect("an archive file");
    let mut files = vec![
        (
            "REPLICATION_SEQUENCE".to_owned(),
            format!("{DUMPED_AT_SEQUENCE}\n"),
        ),
        ("SCHEMA_SEQUENCE".to_owned(), format!("{SCHEMA}\n")),
    ];
    for (name, rows) in tables {
        let mut body = rows.join("\n");
        if !body.is_empty() {
            body.push('\n');
        }
        files.push((format!("mbdump/{name}"), body));
    }
    archived(file, &files);
}

fn dump(into: &Path) -> Archives {
    let core = into.join("mbdump.tar.bz2");
    let derived = into.join("mbdump-derived.tar.bz2");
    archive(
        &core,
        &[
            ("area", vec![line(14, &[(0, "10"), (2, "United Kingdom")])]),
            (
                "artist",
                vec![
                    line(
                        19,
                        &[
                            (0, "1"),
                            (1, RADIOHEAD),
                            (2, "Radiohead"),
                            (3, "Radiohead"),
                            (4, "1985"),
                            (10, "2"),
                            (11, "10"),
                            (13, ""),
                            (16, "f"),
                        ],
                    ),
                    line(
                        19,
                        &[
                            (0, "2"),
                            (1, THOM),
                            (2, "Thom Yorke"),
                            (3, "Yorke, Thom"),
                            (12, "1"),
                            (13, ""),
                            (16, "f"),
                        ],
                    ),
                    line(
                        19,
                        &[
                            (0, "3"),
                            (1, PROLIFIC),
                            (2, "Prolific"),
                            (3, "Prolific"),
                            (13, ""),
                            (16, "f"),
                        ],
                    ),
                ],
            ),
            (
                "artist_alias",
                vec![line(16, &[(1, "1"), (2, "Radio Head")])],
            ),
            (
                "artist_credit_name",
                vec![
                    line(
                        5,
                        &[(0, "1"), (1, "0"), (2, "1"), (3, "Radiohead"), (4, "")],
                    ),
                    line(
                        5,
                        &[(0, "2"), (1, "0"), (2, "2"), (3, "Thom Yorke"), (4, "")],
                    ),
                    line(5, &[(0, "3"), (1, "0"), (2, "3"), (3, "Prolific"), (4, "")]),
                ],
            ),
            ("artist_type", vec![line(6, &[(0, "2"), (1, "Group")])]),
            ("gender", vec![line(6, &[(0, "1"), (1, "Male")])]),
            ("iso_3166_1", vec![line(2, &[(0, "10"), (1, "GB")])]),
            ("isrc", vec![line(5, &[(1, "1"), (2, CREEP_CODE)])]),
            (
                "l_artist_url",
                vec![line(9, &[(1, "1"), (2, "1"), (3, "1")])],
            ),
            ("l_recording_url", vec![]),
            ("l_release_group_url", vec![]),
            ("l_release_url", vec![]),
            ("label", vec![line(16, &[(0, "1"), (2, "Parlophone")])]),
            ("link", vec![line(11, &[(0, "1"), (1, "1")])]),
            ("link_type", vec![line(16, &[(0, "1"), (6, "wikidata")])]),
            (
                "medium",
                vec![line(
                    9,
                    &[(0, "1"), (1, "1"), (2, "1"), (3, "1"), (4, ""), (7, "2")],
                )],
            ),
            ("medium_format", vec![line(8, &[(0, "1"), (1, "CD")])]),
            (
                "recording",
                vec![
                    line(
                        9,
                        &[(0, "1"), (1, CREEP), (2, "Creep"), (3, "1"), (4, "238000")],
                    ),
                    line(
                        9,
                        &[
                            (0, "2"),
                            (1, ACOUSTIC),
                            (2, "Creep (Acoustic)"),
                            (3, "1"),
                            (4, "260000"),
                        ],
                    ),
                    line(
                        9,
                        &[
                            (0, "3"),
                            (1, HEARING),
                            (2, "Hearing Damage"),
                            (3, "2"),
                            (4, "300000"),
                        ],
                    ),
                ],
            ),
            (
                "release",
                [
                    vec![line(
                        14,
                        &[
                            (0, "1"),
                            (1, PABLO),
                            (2, "Pablo Honey"),
                            (3, "1"),
                            (4, "1"),
                            (5, "1"),
                            (9, PABLO_BARCODE),
                            (10, ""),
                        ],
                    )],
                    prolific_releases(),
                ]
                .concat(),
            ),
            (
                "release_country",
                vec![line(
                    5,
                    &[(0, "1"), (1, "10"), (2, "1993"), (3, "2"), (4, "22")],
                )],
            ),
            (
                "release_group",
                [
                    vec![line(
                        8,
                        &[
                            (0, "1"),
                            (1, PABLO_GROUP),
                            (2, "Pablo Honey"),
                            (3, "1"),
                            (4, "1"),
                            (5, ""),
                        ],
                    )],
                    prolific_groups(),
                ]
                .concat(),
            ),
            (
                "release_group_primary_type",
                vec![line(6, &[(0, "1"), (1, "Album")])],
            ),
            (
                "release_group_secondary_type",
                vec![line(6, &[(0, "1"), (1, "Live")])],
            ),
            ("release_group_secondary_type_join", vec![]),
            (
                "release_label",
                vec![line(5, &[(1, "1"), (2, "1"), (3, "CDPCSD 129")])],
            ),
            (
                "release_status",
                vec![line(6, &[(0, "1"), (1, "Official")])],
            ),
            ("release_unknown_country", vec![]),
            (
                "track",
                vec![
                    line(
                        12,
                        &[
                            (0, "1"),
                            (1, TRACK_ONE),
                            (2, "1"),
                            (3, "1"),
                            (4, "1"),
                            (5, "1"),
                            (6, "Creep"),
                            (7, "1"),
                            (8, "238000"),
                        ],
                    ),
                    line(
                        12,
                        &[
                            (0, "2"),
                            (1, TRACK_TWO),
                            (2, "2"),
                            (3, "1"),
                            (4, "2"),
                            (5, "2"),
                            (6, "Creep (Acoustic)"),
                            (7, "1"),
                            (8, "260000"),
                        ],
                    ),
                ],
            ),
            (
                "url",
                vec![line(
                    5,
                    &[(0, "1"), (2, "https://www.wikidata.org/wiki/Q44190")],
                )],
            ),
        ],
    );
    archive(
        &derived,
        &[
            ("artist_tag", vec![line(4, &[(0, "1"), (1, "1"), (2, "5")])]),
            (
                "release_group_meta",
                vec![line(7, &[(0, "1"), (2, "1993"), (3, "2"), (4, "22")])],
            ),
            ("release_meta", vec![line(5, &[(0, "1"), (4, "present")])]),
            ("tag", vec![line(3, &[(0, "1"), (1, "alternative rock")])]),
        ],
    );
    Archives {
        core,
        derived,
        dump: "20261104-002222".to_owned(),
    }
}

fn mbid(text: &str) -> Mbid {
    Mbid::new(text).expect("a well-formed mbid")
}

fn built_keeping(scratch: &Scratch, keeping: Keeping) -> Mirror {
    let archives = dump(&scratch.0);
    let folder = scratch.0.join("musicbrainz");
    let held = held_for_building(&folder).expect("the building lock");
    let building = Building::default();
    let built = built_from(&archives, &folder, keeping, &building, &held).expect("a built mirror");
    drop(held);
    let settled = settled(&built, &folder).expect("the mirror settled");
    Mirror::open(&settled).expect("the mirror opens")
}

fn built(scratch: &Scratch) -> Mirror {
    built_keeping(scratch, Keeping::TheMirror)
}

#[test]
fn a_mirror_built_from_the_dump_answers_every_lookup_by_id() {
    let scratch = Scratch::new("lookups");
    let mirror = built(&scratch);

    let release = mirror
        .release(&mbid(PABLO))
        .expect("a read")
        .expect("the release");
    let recording = mirror
        .recording(&mbid(CREEP))
        .expect("a read")
        .expect("the recording");
    let group = mirror
        .release_group(&mbid(PABLO_GROUP))
        .expect("a read")
        .expect("the group");
    let artist = mirror
        .artist(&mbid(RADIOHEAD))
        .expect("a read")
        .expect("the artist");

    assert_eq!(mirror.made().dump, "20261104-002222");
    assert_eq!(release.title, "Pablo Honey");
    assert_eq!(release.credit[0].mbid, Some(mbid(RADIOHEAD)));
    assert_eq!(release.date.as_deref(), Some("1993-02-22"));
    assert_eq!(release.country.as_deref(), Some("GB"));
    assert_eq!(release.label.as_deref(), Some("Parlophone"));
    assert_eq!(release.catalog_number.as_deref(), Some("CDPCSD 129"));
    assert_eq!(release.barcode.as_deref(), Some(PABLO_BARCODE));
    assert_eq!(release.kind.as_deref(), Some("Album"));
    assert_eq!(release.group, Some(mbid(PABLO_GROUP)));
    assert!(release.has_front_cover);
    assert_eq!(release.media[0].format.as_deref(), Some("CD"));
    assert_eq!(release.track_count(), 2);
    assert_eq!(release.media[0].tracks[0].recording, Some(mbid(CREEP)));
    assert_eq!(release.media[0].tracks[0].track, Some(mbid(TRACK_ONE)));
    assert_eq!(
        release.media[0].tracks[0].isrcs,
        vec![CREEP_CODE.to_owned()]
    );
    assert_eq!(
        release.media[0].tracks[1].length,
        Some(Duration::from_secs(260))
    );
    assert_eq!(release.media[0].tracks[0].artist, None);

    assert_eq!(recording.title, "Creep");
    assert_eq!(recording.length, Some(Duration::from_secs(238)));
    assert_eq!(
        recording.isrcs,
        vec![Isrc::new(CREEP_CODE).expect("a code")]
    );
    assert_eq!(recording.releases[0].id, mbid(PABLO));
    assert_eq!(recording.releases[0].disc, Some(1));
    assert_eq!(recording.releases[0].position, Some(1));
    assert_eq!(recording.releases[0].issued.kind.as_deref(), Some("Album"));
    assert_eq!(
        recording.releases[0].issued.status.as_deref(),
        Some("Official")
    );

    assert_eq!(group.first_released.as_deref(), Some("1993-02-22"));
    assert_eq!(group.releases[0].track_count, Some(2));
    assert_eq!(group.releases[0].discs, vec![2]);

    assert_eq!(artist.kind.as_deref(), Some("Group"));
    assert_eq!(artist.country.as_deref(), Some("GB"));
    assert_eq!(artist.area.as_deref(), Some("United Kingdom"));
    assert_eq!(artist.span.begin.as_deref(), Some("1985"));
    assert_eq!(artist.aliases, vec!["Radio Head".to_owned()]);
    assert_eq!(artist.genres[0].name, "alternative rock");
    assert_eq!(artist.links[0].url, "https://www.wikidata.org/wiki/Q44190");
}

#[test]
fn a_mirror_answers_codes_barcodes_and_what_an_artist_and_a_group_hold() {
    let scratch = Scratch::new("browse");
    let mirror = built(&scratch);

    let coded = mirror
        .recordings_of_isrc(&Isrc::new(CREEP_CODE).expect("a code"))
        .expect("a read");
    let barcoded = mirror
        .releases_by_barcode(&Barcode::new(PABLO_BARCODE).expect("a barcode"))
        .expect("a read");
    let discography = mirror
        .release_groups_of(&mbid(RADIOHEAD), 0)
        .expect("a read");
    let pressings = mirror
        .releases_of_group(&mbid(PABLO_GROUP))
        .expect("a read");
    let pressed = mirror
        .releases_of_artist(&mbid(RADIOHEAD), 0)
        .expect("a read");
    let none = mirror.release_groups_of(&mbid(THOM), 0).expect("a read");

    assert_eq!(coded.len(), 1);
    assert_eq!(coded[0].id, mbid(CREEP));
    assert_eq!(barcoded.len(), 1);
    assert_eq!(barcoded[0].release, mbid(PABLO));
    assert_eq!(discography.releases.len(), 1);
    assert_eq!(discography.releases[0].mbid, mbid(PABLO_GROUP));
    assert_eq!(discography.unread, 0);
    assert_eq!(discography.read_to, 1);
    assert_eq!(pressings.len(), 1);
    assert_eq!(pressed.pressings.len(), 1);
    assert_eq!(pressed.credited, 1);
    assert!(none.releases.is_empty());
}

#[test]
fn an_artists_pressings_are_paged_the_same_whether_or_not_its_list_was_already_read() {
    let scratch = Scratch::new("paging");
    let mirror = built(&scratch);
    let artist = mbid(PROLIFIC);
    let everything = PROLIFIC_GROUPS * PRESSINGS_IN_A_PROLIFIC_GROUP;

    let first = mirror.releases_of_artist(&artist, 0).expect("a read");
    let second = mirror
        .releases_of_artist(&artist, first.read_to)
        .expect("a read");
    let again = mirror.releases_of_artist(&artist, 0).expect("a read");
    let cold = Mirror::open(mirror.path())
        .expect("the mirror opens again")
        .releases_of_artist(&artist, first.read_to)
        .expect("a read");
    let past = mirror
        .releases_of_artist(&artist, u32::MAX)
        .expect("a read");
    let stranger = mirror.releases_of_artist(&mbid(CREEP), 0).expect("a read");

    let ids = |page: &ArtistPressings| -> Vec<Mbid> {
        page.pressings
            .iter()
            .map(|pressing| pressing.id.clone())
            .collect()
    };
    let mut every: Vec<Mbid> = [ids(&first), ids(&second)].concat();
    every.sort();
    every.dedup();

    assert_eq!(first.credited as usize, everything);
    assert_eq!(first.pressings.len(), ARTIST_PRESSINGS_PAGE);
    assert_eq!(first.read_to as usize, ARTIST_PRESSINGS_PAGE);
    assert_eq!(second.credited as usize, everything);
    assert_eq!(second.pressings.len(), everything - ARTIST_PRESSINGS_PAGE);
    assert_eq!(second.read_to as usize, everything);
    assert_eq!(every.len(), everything);
    assert_eq!(ids(&again), ids(&first));
    assert_eq!(again.read_to, first.read_to);
    assert_eq!(ids(&cold), ids(&second));
    assert_eq!(cold.read_to, second.read_to);
    assert!(past.pressings.is_empty());
    assert_eq!(past.read_to as usize, everything);
    assert!(stranger.pressings.is_empty());
}

#[test]
fn a_local_search_scores_whole_agreement_and_nothing_less_at_a_hundred() {
    let scratch = Scratch::new("search");
    let mirror = built(&scratch);

    let exact = mirror
        .find_recording(&RecordingAsked {
            title: "Creep".to_owned(),
            artist: Some("Radiohead".to_owned()),
            artist_mbid: None,
            release: None,
            length: Some(Duration::from_secs(238)),
            wording: Wording::Phrase,
        })
        .expect("a search");
    let other_credit = mirror
        .find_recording(&RecordingAsked {
            title: "Creep".to_owned(),
            artist: Some("Thom Yorke".to_owned()),
            artist_mbid: None,
            release: None,
            length: None,
            wording: Wording::Words,
        })
        .expect("a search");
    let releases = mirror
        .find_release(&ReleaseAsked {
            title: "pablo honey".to_owned(),
            artist: Some("Radiohead".to_owned()),
            artist_mbid: None,
            barcode: Some(PABLO_BARCODE.to_owned()),
            catalog_number: None,
            wording: Wording::Phrase,
        })
        .expect("a search");
    let groups = mirror
        .find_release_group(&GroupAsked {
            title: "Pablo Honey".to_owned(),
            artist: None,
            artist_mbid: Some(mbid(RADIOHEAD)),
            year: Some(1993),
            wording: Wording::Words,
        })
        .expect("a search");
    let another_year = mirror
        .find_release_group(&GroupAsked {
            title: "Pablo Honey".to_owned(),
            artist: Some("Radiohead".to_owned()),
            artist_mbid: None,
            year: Some(1990),
            wording: Wording::Phrase,
        })
        .expect("a search");
    let artists = mirror.find_artist("radio head").expect("a search");
    let songs = mirror
        .find_songs(&SongsAsked {
            words: "creep radiohead".to_owned(),
            typed: "creep radiohead".to_owned(),
            by: None,
        })
        .expect("a search");
    let albums = mirror.find_albums("pablo honey").expect("a search");

    assert_eq!(exact.len(), 1);
    assert_eq!(exact[0].recording, mbid(CREEP));
    assert_eq!(exact[0].score, 100);
    assert!(other_credit.iter().all(|found| found.score < 95));
    assert_eq!(releases[0].release, mbid(PABLO));
    assert_eq!(releases[0].score, 100);
    assert_eq!(releases[0].track_count, Some(2));
    assert_eq!(groups[0].group, mbid(PABLO_GROUP));
    assert_eq!(groups[0].score, 100);
    assert!(another_year.is_empty());
    assert_eq!(artists[0].mbid, mbid(RADIOHEAD));
    assert_eq!(artists[0].score, 100);
    assert_eq!(songs[0].recording, mbid(CREEP));
    assert_eq!(albums[0].group, mbid(PABLO_GROUP));
}

#[test]
fn a_layered_reference_keeps_what_the_mirror_answered_and_serves_it_without_the_mirror() {
    let scratch = Scratch::new("layered");
    let mirror = Arc::new(built(&scratch));
    let kept = Arc::new(Kept::in_memory().expect("a kept store"));
    let mirrored = Arc::new(Mirrored::default());
    mirrored.hold(Some(Arc::clone(&mirror)));

    let layered = Layered::new()
        .keeping(Some(Arc::clone(&kept)))
        .mirrored(Some(Arc::clone(&mirrored)));
    let first = layered.answered(&Asked::Recording(mbid(CREEP)));
    mirrored.hold(None);
    let again = layered.answered(&Asked::Recording(mbid(CREEP)));
    let unknown = layered
        .release(&mbid(HEARING))
        .expect("a miss is no failure");

    assert_eq!(first.as_ref().map(|(layer, _)| *layer), Some(Layer::Mirror));
    assert_eq!(again.as_ref().map(|(layer, _)| *layer), Some(Layer::Kept));
    assert_eq!(
        first.map(|(_, answer)| answer),
        again.map(|(_, answer)| answer)
    );
    assert_eq!(unknown, None);
}

#[test]
fn a_device_holding_a_mirror_answers_what_another_asks_and_a_device_without_answers_from_what_it_kept()
 {
    let scratch = Scratch::new("consulted");
    let mirror = Arc::new(built(&scratch));
    let mirrored = Arc::new(Mirrored::default());
    mirrored.hold(Some(mirror));
    let serving = Consultation::new(Some(mirrored), None);
    let kept = Arc::new(Kept::in_memory().expect("a kept store"));
    let keeping = Consultation::new(None, Some(Arc::clone(&kept)));

    let asked = Asked::Release(mbid(PABLO));
    let answered = serving
        .consulted(&asked.encoded())
        .map(|bytes| Answer::decoded(&bytes).expect("an answer"));
    let before = keeping.consulted(&asked.encoded());
    kept.keep(&asked, answered.as_ref().expect("an answer"))
        .expect("kept");
    let after = keeping
        .consulted(&asked.encoded())
        .map(|bytes| Answer::decoded(&bytes).expect("an answer"));

    assert!(matches!(&answered, Some(Answer::Release(Some(release))) if release.id == mbid(PABLO)));
    assert_eq!(before, None);
    assert_eq!(after, answered);
    assert_eq!(serving.consulted(b"\x01\xff"), None);
}

const REMASTERED: &str = "0a1ac1a2-6b8f-4e2a-9bc6-1f1c6f9d0004";
const TRACK_THREE: &str = "1d1e1f10-0000-4000-8000-000000000003";
const REMASTERED_CODE: &str = "GBAYE1500001";

fn pending(seq: u64, table: &str, xid: u64, old: Option<&str>, new: Option<&str>) -> String {
    let op = match (old, new) {
        (None, Some(_)) => "i",
        (Some(_), None) => "d",
        _ => "u",
    };
    let copied = |json: Option<&str>| {
        json.map_or_else(|| "\\N".to_owned(), |json| json.replace('\\', "\\\\"))
    };
    format!(
        "{seq}\tmusicbrainz.{table}\t{op}\t{xid}\t{}\t{}\t\\N\t\\N",
        copied(old),
        copied(new)
    )
}

fn packet(sequence: u64, schema: u32, changes: &[String]) -> Packet {
    let mut body = changes.join("\n");
    body.push('\n');
    let mut packed = Vec::new();
    archived(
        &mut packed,
        &[
            ("REPLICATION_SEQUENCE".to_owned(), format!("{sequence}\n")),
            ("SCHEMA_SEQUENCE".to_owned(), format!("{schema}\n")),
            ("DBMIRROR_VERSION".to_owned(), "2\n".to_owned()),
            (
                "TIMESTAMP".to_owned(),
                "2026-11-11 01:00:00+00\n".to_owned(),
            ),
            ("mbdump/pending_data".to_owned(), body),
            ("mbdump/pending_keys".to_owned(), String::new()),
        ],
    );
    packet_read(packed.as_slice(), sequence).expect("a packet read")
}

fn recording_json(id: u32, gid: &str, name: &str, length: u32) -> String {
    format!(
        r#"{{"id":{id},"gid":"{gid}","name":"{name}","artist_credit":1,"length":{length},"comment":"","edits_pending":0,"last_updated":"2026-11-11T00:00:00+00:00","video":false}}"#
    )
}

fn first_packet() -> Packet {
    packet(
        DUMPED_AT_SEQUENCE + 1,
        SCHEMA,
        &[
            pending(
                1,
                "recording",
                7,
                Some(&recording_json(1, CREEP, "Creep", 238_000)),
                Some(&recording_json(
                    1,
                    CREEP,
                    "Creep (Remastered 2009)",
                    238_000,
                )),
            ),
            pending(
                2,
                "recording",
                8,
                None,
                Some(&recording_json(4, REMASTERED, "Stop Whispering", 326_000)),
            ),
            pending(
                3,
                "isrc",
                8,
                None,
                Some(&format!(
                    r#"{{"id":9,"recording":4,"isrc":"{REMASTERED_CODE}","source":null,"edits_pending":0,"created":null}}"#
                )),
            ),
            pending(
                4,
                "track",
                8,
                None,
                Some(&format!(
                    r#"{{"id":3,"gid":"{TRACK_THREE}","recording":4,"medium":1,"position":3,"number":"3","name":"Stop Whispering","artist_credit":1,"length":326000,"edits_pending":0,"last_updated":null,"is_data_track":false}}"#
                )),
            ),
            pending(
                5,
                "medium",
                8,
                Some(
                    r#"{"id":1,"release":1,"position":1,"format":1,"name":"","edits_pending":0,"last_updated":null,"track_count":2,"gid":"9d9d9d9d-0000-4000-8000-000000000001"}"#,
                ),
                Some(
                    r#"{"id":1,"release":1,"position":1,"format":1,"name":"","edits_pending":0,"last_updated":null,"track_count":3,"gid":"9d9d9d9d-0000-4000-8000-000000000001"}"#,
                ),
            ),
            pending(
                6,
                "recording",
                9,
                Some(&recording_json(3, HEARING, "Hearing Damage", 300_000)),
                None,
            ),
            pending(
                7,
                "release_country",
                10,
                Some(r#"{"release":1,"country":10,"date_year":1993,"date_month":2,"date_day":22}"#),
                Some(r#"{"release":1,"country":10,"date_year":1993,"date_month":4,"date_day":20}"#),
            ),
            pending(
                8,
                "url",
                11,
                Some(
                    r#"{"id":1,"gid":"5e5e5e5e-0000-4000-8000-000000000001","url":"https://www.wikidata.org/wiki/Q44190","edits_pending":0,"last_updated":null}"#,
                ),
                Some(
                    r#"{"id":1,"gid":"5e5e5e5e-0000-4000-8000-000000000001","url":"https://www.wikidata.org/wiki/Q44191","edits_pending":0,"last_updated":null}"#,
                ),
            ),
        ],
    )
}

fn replicated_through(
    mirror: &Mirror,
    folder: &Path,
    packets: Vec<Packet>,
) -> resonate_musicbrainz::Result<resonate_musicbrainz::Replicated> {
    let held = held_for_building(folder).expect("the building lock");
    let mut packets = packets.into_iter();
    replicated(
        folder,
        mirror,
        |asked| {
            Ok(packets
                .next()
                .inspect(|packet| assert_eq!(packet.sequence, asked)))
        },
        &Building::default(),
        &held,
    )
}

#[test]
fn a_replication_packet_rewrites_the_documents_its_changes_touch_and_nothing_is_built_again() {
    let scratch = Scratch::new("replicated");
    let folder = scratch.0.join("musicbrainz");
    let mirror = built_keeping(&scratch, Keeping::TheMirrorAndItsTables);

    let done = replicated_through(&mirror, &folder, vec![first_packet()]).expect("replicated");
    let amended = Mirror::open(mirror.path()).expect("the mirror opens again");

    let renamed = amended
        .recording(&mbid(CREEP))
        .expect("a read")
        .expect("the recording");
    let added = amended
        .recording(&mbid(REMASTERED))
        .expect("a read")
        .expect("the new recording");
    let gone = amended.recording(&mbid(HEARING)).expect("a read");
    let release = amended
        .release(&mbid(PABLO))
        .expect("a read")
        .expect("the release");
    let group = amended
        .release_group(&mbid(PABLO_GROUP))
        .expect("a read")
        .expect("the group");
    let artist = amended
        .artist(&mbid(RADIOHEAD))
        .expect("a read")
        .expect("the artist");
    let coded = amended
        .recordings_of_isrc(&Isrc::new(REMASTERED_CODE).expect("a code"))
        .expect("a read");
    let barcoded = amended
        .releases_by_barcode(&Barcode::new(PABLO_BARCODE).expect("a barcode"))
        .expect("a read");
    let found = amended
        .find_recording(&RecordingAsked {
            title: "Creep (Remastered 2009)".to_owned(),
            artist: Some("Radiohead".to_owned()),
            artist_mbid: None,
            release: None,
            length: None,
            wording: Wording::Phrase,
        })
        .expect("a search");
    let old_title = amended
        .find_recording(&RecordingAsked {
            title: "Creep".to_owned(),
            artist: Some("Radiohead".to_owned()),
            artist_mbid: None,
            release: None,
            length: None,
            wording: Wording::Words,
        })
        .expect("a search");
    let untouched = amended
        .recording(&mbid(ACOUSTIC))
        .expect("a read")
        .expect("an untouched recording");

    assert_eq!(done.from, DUMPED_AT_SEQUENCE);
    assert_eq!(done.to, DUMPED_AT_SEQUENCE + 1);
    assert_eq!(done.changes, 8);
    assert_eq!(
        amended.amended_to().map(|to| to.sequence),
        Some(DUMPED_AT_SEQUENCE + 1)
    );
    assert_eq!(renamed.title, "Creep (Remastered 2009)");
    assert_eq!(renamed.releases[0].date.as_deref(), Some("1993-04-20"));
    assert_eq!(added.title, "Stop Whispering");
    assert_eq!(added.releases[0].id, mbid(PABLO));
    assert_eq!(added.releases[0].position, Some(3));
    assert_eq!(gone, None);
    assert_eq!(release.track_count(), 3);
    assert_eq!(release.date.as_deref(), Some("1993-04-20"));
    assert_eq!(release.media[0].tracks[2].recording, Some(mbid(REMASTERED)));
    assert_eq!(group.releases[0].discs, vec![3]);
    assert_eq!(group.releases[0].date.as_deref(), Some("1993-04-20"));
    assert_eq!(artist.links[0].url, "https://www.wikidata.org/wiki/Q44191");
    assert_eq!(coded.len(), 1);
    assert_eq!(coded[0].id, mbid(REMASTERED));
    assert_eq!(barcoded.len(), 1);
    assert_eq!(found[0].recording, mbid(CREEP));
    assert_eq!(found[0].score, 100);
    assert_eq!(
        old_title
            .iter()
            .filter(|found| found.recording == mbid(CREEP))
            .count(),
        1
    );
    assert_eq!(untouched.title, "Creep (Acoustic)");
}

#[test]
fn a_packet_of_another_schema_stops_replication_where_it_stood_and_a_new_build_lets_the_changes_go()
{
    let scratch = Scratch::new("schema");
    let folder = scratch.0.join("musicbrainz");
    let mirror = built_keeping(&scratch, Keeping::TheMirrorAndItsTables);

    let refused = replicated_through(
        &mirror,
        &folder,
        vec![packet(DUMPED_AT_SEQUENCE + 1, SCHEMA + 1, &[])],
    );
    let nothing_new = replicated_through(&mirror, &folder, Vec::new()).expect("nothing to do");
    let first = replicated_through(&mirror, &folder, vec![first_packet()]).expect("replicated");
    let amended_before = folder.join(AMENDED_FILE).is_file();
    let rebuilt = built_keeping(&scratch, Keeping::TheMirror);

    assert!(matches!(
        refused,
        Err(Error::PacketSchema {
            sequence: 101,
            found: 32,
            reads: 31
        })
    ));
    assert_eq!(nothing_new.to, DUMPED_AT_SEQUENCE);
    assert_eq!(first.to, DUMPED_AT_SEQUENCE + 1);
    assert!(amended_before);
    assert!(!folder.join(AMENDED_FILE).is_file());
    assert!(!folder.join(TABLES_FILE).is_file());
    assert_eq!(rebuilt.amended_to(), None);
    assert_eq!(
        rebuilt
            .recording(&mbid(CREEP))
            .expect("a read")
            .map(|recording| recording.title),
        Some("Creep".to_owned())
    );
}

#[test]
fn a_mirror_built_without_its_tables_cannot_follow_the_changes() {
    let scratch = Scratch::new("untabled");
    let folder = scratch.0.join("musicbrainz");
    let mirror = built(&scratch);

    let refused = replicated_through(&mirror, &folder, vec![first_packet()]);

    assert!(matches!(refused, Err(Error::NoTables { .. })));
}

#[test]
fn a_packet_naming_another_number_than_the_one_asked_for_is_refused() {
    let mut packed = Vec::new();
    archived(
        &mut packed,
        &[
            ("REPLICATION_SEQUENCE".to_owned(), "7\n".to_owned()),
            ("SCHEMA_SEQUENCE".to_owned(), "31\n".to_owned()),
            ("DBMIRROR_VERSION".to_owned(), "2\n".to_owned()),
            ("mbdump/pending_data".to_owned(), String::new()),
        ],
    );
    let mut old_form = Vec::new();
    archived(
        &mut old_form,
        &[
            ("REPLICATION_SEQUENCE".to_owned(), "8\n".to_owned()),
            ("SCHEMA_SEQUENCE".to_owned(), "31\n".to_owned()),
            ("mbdump/pending_data".to_owned(), String::new()),
        ],
    );

    let other = packet_read(packed.as_slice(), 8).err();
    let first_form = packet_read(old_form.as_slice(), 8).err();

    assert!(matches!(
        other,
        Some(Error::PacketSequence { asked: 8, found: 7 })
    ));
    assert!(matches!(
        first_form,
        Some(Error::PacketForm { sequence: 8 })
    ));
}
