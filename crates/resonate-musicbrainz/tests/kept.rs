use std::{
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use resonate_core::{Isrc, Link, Mbid, SourceId};
use resonate_library::{
    AlbumLink, AlbumMatch, AlbumNames, ArtistLink, ArtistMatch, ArtistPressings, ArtistProfile,
    Barcode, BarcodeMatch, CoverArt, Credit, Discography, GroupAsked, GroupMatch, Issued,
    LinkNames, LinkedPlaylist, LinkedProfile, LyricText, LyricsAsked, PlaylistLink, ProfileLink,
    Recording, RecordingAsked, RecordingMatch, RecordingRelease, Reference, Release, ReleaseAsked,
    ReleaseGroup, ReleaseMatch, SongLink, SongsAsked, StreamAsked, Wording,
};
use resonate_musicbrainz::{
    Answer, Asked, Error, KEPT_FRESH_FOR, Kept, KeptAnswer, Layer, Layered, Peers,
};

const ECHOES: &str = "83d91898-7763-47d7-b03b-b92132375c47";
const MEDDLE: &str = "a5bdf1b3-0cd8-3a1e-9e3a-0a4b1a4c4f0e";
const DESK: &str = "0123456789abcdef0123456789abcdef";

fn mbid(text: &str) -> Mbid {
    Mbid::new(text).expect("a well-formed mbid")
}

fn echoes() -> Recording {
    Recording {
        id: mbid(ECHOES),
        title: "Echoes".to_owned(),
        credit: vec![Credit {
            name: "Pink Floyd".to_owned(),
            joined_by: String::new(),
            mbid: None,
        }],
        length: Some(Duration::from_secs(1_403)),
        isrcs: vec![Isrc::new("GBN9Y7100006").expect("a code")],
        releases: vec![RecordingRelease {
            id: mbid(MEDDLE),
            title: "Meddle".to_owned(),
            date: Some("1971-10-30".to_owned()),
            disc: Some(1),
            position: Some(6),
            issued: Issued {
                kind: Some("Album".to_owned()),
                secondary: Vec::new(),
                status: Some("Official".to_owned()),
            },
        }],
    }
}

fn scratch(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!("resonate-kept-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).expect("a scratch folder");
    folder
}

#[test]
fn every_lookup_and_answer_reads_back_as_itself_and_a_cut_one_is_refused() {
    let asked = [
        Asked::Recording(mbid(ECHOES)),
        Asked::Isrc(Isrc::new("GBN9Y7100006").expect("a code")),
        Asked::FindRecording(RecordingAsked {
            title: "Echoes".to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            artist_mbid: None,
            release: Some("Meddle".to_owned()),
            length: Some(Duration::from_secs(1_403)),
            wording: Wording::Phrase,
        }),
        Asked::FindGroup(GroupAsked {
            title: "Meddle".to_owned(),
            artist: None,
            artist_mbid: Some(mbid(ECHOES)),
            year: Some(1971),
            wording: Wording::Words,
        }),
        Asked::Barcode(Barcode::new("077774638320").expect("a barcode")),
        Asked::GroupsOf {
            artist: mbid(ECHOES),
            from: 1_000,
        },
    ];
    let answer = Answer::Recording(Some(echoes()));
    let encoded = answer.encoded();

    for asked in asked {
        assert_eq!(Asked::decoded(&asked.encoded()).expect("decoded"), asked);
    }
    assert_eq!(Answer::decoded(&encoded).expect("decoded"), answer);
    assert!(Answer::decoded(&encoded[..encoded.len() - 1]).is_err());
    assert!(Answer::decoded(&[encoded.as_slice(), &[0]].concat()).is_err());
}

#[test]
fn a_search_is_kept_under_its_folded_words_and_a_miss_is_never_kept() {
    let kept = Kept::in_memory().expect("a kept store");
    let asked = |title: &str| {
        Asked::FindRecording(RecordingAsked {
            title: title.to_owned(),
            artist: Some("Pink Floyd".to_owned()),
            artist_mbid: None,
            release: None,
            length: None,
            wording: Wording::Words,
        })
    };
    let found = Answer::RecordingMatches(vec![RecordingMatch::from(echoes())]);

    let missed = kept
        .keep(&asked("Echoes"), &Answer::RecordingMatches(Vec::new()))
        .expect("asked");
    let otherwise = kept
        .keep(&Asked::Recording(mbid(ECHOES)), &found)
        .expect("asked");
    let held = kept.keep(&asked("Echoes"), &found).expect("kept");

    assert!(!missed);
    assert!(!otherwise);
    assert!(held);
    assert_eq!(kept.answer(&asked("  ÉCHOES ")).expect("read"), Some(found));
}

#[test]
fn an_entity_learnt_inside_another_answer_answers_a_lookup_by_its_id() {
    let kept = Kept::in_memory().expect("a kept store");
    let coded = Asked::Isrc(Isrc::new("GBN9Y7100006").expect("a code"));

    kept.keep(&coded, &Answer::Recordings(vec![echoes()]))
        .expect("kept");

    assert_eq!(
        kept.answer(&Asked::Recording(mbid(ECHOES))).expect("read"),
        Some(Answer::Recording(Some(echoes())))
    );
    assert_eq!(
        kept.answer(&Asked::Recording(mbid(MEDDLE))).expect("read"),
        None
    );
}

#[test]
fn a_kept_answer_older_than_a_month_is_not_served() {
    let kept = Kept::in_memory().expect("a kept store");
    let asked = Asked::Recording(mbid(ECHOES));
    kept.keep(&asked, &Answer::Recording(Some(echoes())))
        .expect("kept");
    let later = resonate_musicbrainz_now() + KEPT_FRESH_FOR.as_secs() + 1;

    assert!(kept.answer(&asked).expect("read").is_some());
    assert_eq!(kept.answer_at(&asked, later).expect("read"), None);
}

fn resonate_musicbrainz_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

#[test]
fn answers_taken_from_a_device_are_marked_and_never_told_back_to_it() {
    let desk = Kept::in_memory().expect("a kept store");
    let phone = Kept::in_memory().expect("a kept store");
    let asked = Asked::Recording(mbid(ECHOES));
    desk.keep(&asked, &Answer::Recording(Some(echoes())))
        .expect("kept");
    let told = desk.after(0, 512, "phone").expect("listed");
    let forged = KeptAnswer {
        key: Asked::Recording(mbid(MEDDLE)).key(),
        ..told[0].clone()
    };

    let taken = phone
        .take(DESK, &[told[0].clone(), forged], told[0].seq)
        .expect("taken");

    assert_eq!(taken, 1);
    assert_eq!(phone.mark_of(DESK).expect("read"), told[0].seq);
    assert_eq!(
        phone.answer(&asked).expect("read"),
        Some(Answer::Recording(Some(echoes())))
    );
    assert!(phone.after(0, 512, DESK).expect("listed").is_empty());
    assert_eq!(phone.after(0, 512, "laptop").expect("listed").len(), 1);
    assert_eq!(phone.take(DESK, &told, told[0].seq).expect("taken"), 0);
}

const TIME: &str = "1b4ad7c2-1e2f-4b8e-9a39-2d44c35a0c11";
const BREATHE: &str = "6e1c5f4a-8d0b-4f5e-b8c2-7f3e1d9a4b22";

fn recorded(id: &str) -> (Asked, Answer) {
    (
        Asked::Recording(mbid(id)),
        Answer::Recording(Some(Recording {
            id: mbid(id),
            ..echoes()
        })),
    )
}

#[test]
fn an_answers_number_never_goes_back_when_the_newest_is_swept_so_a_device_past_it_is_told_the_next()
{
    let folder = scratch("numbered");
    let path = folder.join("kept.sqlite");
    let desk = Kept::open(&path).expect("laid out");
    for id in [ECHOES, TIME] {
        let (asked, answer) = recorded(id);
        desk.keep(&asked, &answer).expect("kept");
    }
    let mark = desk
        .after(0, 512, "phone")
        .expect("listed")
        .iter()
        .map(|told| told.seq)
        .max()
        .expect("answers told");
    drop(desk);
    rusqlite::Connection::open(&path)
        .expect("opened")
        .execute("UPDATE answers SET at = 0 WHERE seq = ?1", [mark as i64])
        .expect("the newest answer aged");

    let desk = Kept::open(&path).expect("opened again, sweeping the newest");
    let (asked, answer) = recorded(BREATHE);
    desk.keep(&asked, &answer).expect("kept");
    let told = desk.after(mark, 512, "phone").expect("listed");

    assert_eq!(desk.counted().expect("counted"), 2);
    assert_eq!(
        told.len(),
        1,
        "an answer kept after a sweep was numbered under a mark passed"
    );
    assert_eq!(told[0].key, asked.key());
    assert!(told[0].seq > mark);
    let _ = fs::remove_dir_all(&folder);
}

#[test]
fn a_device_holding_a_mark_past_every_number_here_is_told_what_is_kept_next() {
    let desk = Kept::in_memory().expect("a kept store");
    let (echoes_asked, echoes_answer) = recorded(ECHOES);
    desk.keep(&echoes_asked, &echoes_answer).expect("kept");

    let beyond = desk.after(u64::MAX, 512, "laptop").expect("listed");
    let unmoved = desk.last_seq().expect("read");
    let passed = desk.after(7, 512, "phone").expect("listed");
    let (asked, answer) = recorded(TIME);
    desk.keep(&asked, &answer).expect("kept");
    let told = desk.after(7, 512, "phone").expect("listed");

    assert!(beyond.is_empty());
    assert_eq!(
        unmoved, 1,
        "a mark no device could hold moved the numbering"
    );
    assert!(passed.is_empty());
    assert_eq!(
        told.len(),
        1,
        "an answer kept after a mark past this store was never told"
    );
    assert_eq!(told[0].seq, 8);
    assert_eq!(told[0].key, asked.key());
}

#[test]
fn kept_answers_laid_out_by_a_later_build_are_refused_and_this_builds_open_again() {
    let folder = scratch("versions");
    let path = folder.join("kept.sqlite");
    {
        let kept = Kept::open(&path).expect("laid out");
        kept.keep(
            &Asked::Recording(mbid(ECHOES)),
            &Answer::Recording(Some(echoes())),
        )
        .expect("kept");
    }
    let again = Kept::open(&path).expect("opened again");
    let held = again.counted().expect("counted");
    drop(again);
    {
        let connection = rusqlite::Connection::open(&path).expect("opened");
        connection
            .pragma_update(None, "user_version", 99)
            .expect("stamped");
    }

    assert_eq!(held, 1);
    assert!(matches!(
        Kept::open(&path),
        Err(Error::KeptVersion { found: 99 })
    ));
    let _ = fs::remove_dir_all(&folder);
}

struct Online {
    source: SourceId,
    asked: AtomicUsize,
}

impl Default for Online {
    fn default() -> Self {
        Self {
            source: SourceId::new("musicbrainz").expect("a source name"),
            asked: AtomicUsize::new(0),
        }
    }
}

impl Reference for Online {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn release(&self, _: &Mbid) -> resonate_library::Result<Option<Release>> {
        Ok(None)
    }

    fn find_release(&self, _: &ReleaseAsked) -> resonate_library::Result<Vec<ReleaseMatch>> {
        Ok(Vec::new())
    }

    fn releases_by_barcode(&self, _: &Barcode) -> resonate_library::Result<Vec<BarcodeMatch>> {
        Ok(Vec::new())
    }

    fn recording(&self, _: &Mbid) -> resonate_library::Result<Option<Recording>> {
        self.asked.fetch_add(1, Ordering::Relaxed);
        Ok(Some(echoes()))
    }

    fn recordings_of_isrc(&self, _: &Isrc) -> resonate_library::Result<Vec<Recording>> {
        Ok(Vec::new())
    }

    fn find_recording(&self, _: &RecordingAsked) -> resonate_library::Result<Vec<RecordingMatch>> {
        Ok(Vec::new())
    }

    fn find_songs(&self, _: &SongsAsked) -> resonate_library::Result<Vec<RecordingMatch>> {
        Ok(Vec::new())
    }

    fn release_group(&self, _: &Mbid) -> resonate_library::Result<Option<ReleaseGroup>> {
        Ok(None)
    }

    fn find_release_group(&self, _: &GroupAsked) -> resonate_library::Result<Vec<GroupMatch>> {
        Ok(Vec::new())
    }

    fn find_albums(&self, _: &str) -> resonate_library::Result<Vec<AlbumMatch>> {
        Ok(Vec::new())
    }

    fn group_cover(&self, _: &Mbid) -> resonate_library::Result<Option<CoverArt>> {
        Ok(None)
    }

    fn artist(&self, _: &Mbid) -> resonate_library::Result<Option<ArtistProfile>> {
        Ok(None)
    }

    fn find_artist(&self, _: &str) -> resonate_library::Result<Vec<ArtistMatch>> {
        Ok(Vec::new())
    }

    fn release_groups_of(&self, _: &Mbid, _: u32) -> resonate_library::Result<Discography> {
        Ok(Discography::default())
    }

    fn releases_of_group(&self, _: &Mbid) -> resonate_library::Result<Vec<Release>> {
        Ok(Vec::new())
    }

    fn releases_of_artist(&self, _: &Mbid, _: u32) -> resonate_library::Result<ArtistPressings> {
        Ok(ArtistPressings::default())
    }

    fn cover(&self, _: &Mbid, _: Option<&Mbid>) -> resonate_library::Result<Option<CoverArt>> {
        Ok(None)
    }

    fn portrait(&self, _: &[Link]) -> resonate_library::Result<Option<CoverArt>> {
        Ok(None)
    }

    fn streamed_at(&self, _: &StreamAsked) -> resonate_library::Result<Option<Link>> {
        Ok(None)
    }

    fn lyrics(&self, _: &LyricsAsked) -> resonate_library::Result<Option<LyricText>> {
        Ok(None)
    }

    fn song_linked(&self, _: &SongLink) -> resonate_library::Result<Option<LinkNames>> {
        Ok(None)
    }

    fn album_linked(&self, _: &AlbumLink) -> resonate_library::Result<Option<AlbumNames>> {
        Ok(None)
    }

    fn artist_linked(&self, _: &ArtistLink) -> resonate_library::Result<Option<String>> {
        Ok(None)
    }

    fn playlist_linked(
        &self,
        _: &PlaylistLink,
    ) -> resonate_library::Result<Option<LinkedPlaylist>> {
        Ok(None)
    }

    fn profile_linked(&self, _: &ProfileLink) -> resonate_library::Result<Option<LinkedProfile>> {
        Ok(None)
    }

    fn artist_at(&self, _: &str) -> resonate_library::Result<Option<Mbid>> {
        Ok(None)
    }
}

#[derive(Default)]
struct Desk {
    asked: AtomicUsize,
    holds: bool,
}

impl Peers for Desk {
    fn consult(&self, asked: &Asked) -> Option<Answer> {
        self.asked.fetch_add(1, Ordering::Relaxed);
        (self.holds && matches!(asked, Asked::Recording(_)))
            .then(|| Answer::Recording(Some(echoes())))
    }
}

#[test]
fn a_device_holding_the_answer_is_asked_before_the_network_and_what_either_answers_is_kept() {
    let kept = Arc::new(Kept::in_memory().expect("a kept store"));
    let online = Arc::new(Online::default());
    let desk = Arc::new(Desk {
        holds: true,
        ..Desk::default()
    });
    let layered = Layered::new()
        .keeping(Some(Arc::clone(&kept)))
        .consulting(Some(Arc::clone(&desk) as Arc<dyn Peers>))
        .falling_back_on(Some(Arc::clone(&online) as Arc<dyn Reference>));

    let first = layered.answered(&Asked::Recording(mbid(ECHOES)));
    let again = layered.answered(&Asked::Recording(mbid(ECHOES)));

    assert_eq!(first.map(|(layer, _)| layer), Some(Layer::Peer));
    assert_eq!(again.map(|(layer, _)| layer), Some(Layer::Kept));
    assert_eq!(desk.asked.load(Ordering::Relaxed), 1);
    assert_eq!(online.asked.load(Ordering::Relaxed), 0);
}

#[test]
fn the_network_is_asked_only_where_nothing_nearer_answers_and_its_answer_is_kept() {
    let kept = Arc::new(Kept::in_memory().expect("a kept store"));
    let online = Arc::new(Online::default());
    let desk = Arc::new(Desk::default());
    let layered = Layered::new()
        .keeping(Some(Arc::clone(&kept)))
        .consulting(Some(Arc::clone(&desk) as Arc<dyn Peers>))
        .falling_back_on(Some(Arc::clone(&online) as Arc<dyn Reference>));

    let first = layered.recording(&mbid(ECHOES)).expect("asked");
    let again = layered.recording(&mbid(ECHOES)).expect("asked");

    assert_eq!(first, Some(echoes()));
    assert_eq!(again, Some(echoes()));
    assert_eq!(online.asked.load(Ordering::Relaxed), 1);
    assert_eq!(desk.asked.load(Ordering::Relaxed), 1);
}
