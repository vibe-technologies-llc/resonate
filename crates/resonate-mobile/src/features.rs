use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use resonate_analysis::{BandLevels, Finding, Watch};
use resonate_codec::{ImageFormat, probe};
use resonate_core::{Chromaprint, Mbid, TrackId, folded_letters, words_of_a_name};
use resonate_engine::{Counting, Listening, Tapped};
use resonate_library::{
    Fingerprints, LookupOp, Printed, RecordingAsked, RecordingMatch, RecordingRelease, Reference,
    SongLink, Sounded, TrackToAsk, Wording, agreement, in_the_order_worth_offering,
};
use resonate_online::{Identity, Online};
use resonate_providers::A_LISTING_NAMED_ALIKE_MAY_DIFFER_BY;
use rusqlite::params;

use crate::{
    Error, MobilePlayer, Result, StoreOp, Track, catalog::Catalog,
    catalog_extras::the_listeners_calendar, documents::location, enrichment::Asking,
    lyrics::KeptSheets,
};

pub(crate) struct Services {
    pub(crate) online: Online,
    pub(crate) auto_eq: resonate_online::AutoEq,
    pub(crate) lyrics: KeptSheets,
    analysis: Mutex<Option<Arc<Watch>>>,
    pub(crate) fitting: Mutex<Option<Arc<Watch>>>,
    pub(crate) analyses: Mutex<BTreeMap<u64, Arc<resonate_analysis::Analysis>>>,
    listening: Mutex<(Listening, Option<i64>)>,
    pub(crate) searching: crate::search::Searching,
    pub(crate) watching: crate::watching::Watching,
}

impl Default for Services {
    fn default() -> Self {
        let online = Online::new(Identity::of_this_build());
        online.client().reach(false);
        let auto_eq = resonate_online::AutoEq::new(Arc::clone(online.client()), None);
        Self {
            online,
            auto_eq,
            lyrics: KeptSheets::default(),
            analysis: Mutex::new(None),
            fitting: Mutex::new(None),
            analyses: Mutex::new(BTreeMap::new()),
            listening: Mutex::new((Listening::default(), None)),
            searching: crate::search::Searching::default(),
            watching: crate::watching::Watching::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Enum)]
pub enum TrackOrder {
    #[default]
    Title,
    Album,
    Artist,
    Duration,
    Added,
    Plays,
    Played,
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct TrackFilter {
    pub text: String,
    pub genre: String,
    pub artist: String,
    pub album: String,
    pub minimum_rate: u32,
    pub minimum_bits: u32,
    pub favourites: bool,
    pub unplayed: bool,
    #[uniffi(default = false)]
    pub played: bool,
    pub order: TrackOrder,
    pub descending: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct LibraryStatistics {
    pub songs: u64,
    pub albums: u64,
    pub artists: u64,
    pub duration_ms: u64,
    pub plays: u64,
    pub listened_ms: u64,
    pub top_tracks: Vec<Track>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ListenedTo {
    pub key: String,
    pub name: String,
    pub by: String,
    pub plays: u64,
    pub listened_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ListenedOnDay {
    pub starts: u64,
    pub plays: u64,
    pub listened_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ListeningDetail {
    pub albums: Vec<ListenedTo>,
    pub artists: Vec<ListenedTo>,
    pub days: Vec<ListenedOnDay>,
    pub songs_heard: u64,
    pub first_heard: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LibraryShare {
    pub name: String,
    pub songs: u64,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct LibraryBreakdown {
    pub formats: Vec<LibraryShare>,
    pub rates: Vec<LibraryShare>,
    pub genres: Vec<LibraryShare>,
    pub decades: Vec<LibraryShare>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct AnalysisReport {
    pub track_id: u64,
    pub verdict: String,
    pub peak: f32,
    pub rms: f32,
    pub true_peak: f32,
    pub loudness_lufs: Option<f32>,
    pub dynamic_range: Option<u32>,
    pub clipped: u64,
    pub bits_in_use: Option<u32>,
    pub waveform: Vec<f32>,
    pub spectrum: Vec<f32>,
    pub spectrum_step_hz: f32,
    pub findings: Vec<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct Inspector {
    pub track: Track,
    pub container: String,
    pub sample_format: String,
    pub seekable: bool,
    pub encoder: String,
    pub copyright: String,
    pub comment: String,
    pub isrc: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct SearchSong {
    pub recording: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: Option<u64>,
    pub isrcs: Vec<String>,
    #[uniffi(default)]
    pub release: String,
    #[uniffi(default)]
    pub release_group: String,
    #[uniffi(default)]
    pub artist_id: String,
    #[uniffi(default)]
    pub disc: Option<u32>,
    #[uniffi(default)]
    pub position: Option<u32>,
    #[uniffi(default)]
    pub held: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RecognitionCandidate {
    pub score: u32,
    pub song: SearchSong,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ArtistInformation {
    pub name: String,
    pub kind: String,
    pub country: String,
    pub area: String,
    pub genres: Vec<String>,
    pub links: Vec<String>,
    pub picture: Option<Vec<u8>>,
}

#[uniffi::export]
pub fn default_track_filter() -> TrackFilter {
    TrackFilter::default()
}

pub(crate) fn recognised_through(
    catalog: &Catalog,
    printer: &dyn Fingerprints,
    id: u64,
    track: Track,
    print: Chromaprint,
) -> Result<Printed> {
    let named = named_on_the_phone(id, &track)?;
    let sounded = Sounded {
        location: location(&track.uri)?,
        span: None,
        length: Some(Duration::from_millis(track.duration_ms)),
        title: Some(track.title),
        artist: Some(track.artist),
        print,
    };
    let recognised = printer
        .recognise(&sounded)
        .map_err(|source| Error::reference(LookupOp::Recognise, source))?;
    let heard = match &recognised {
        Printed::Nothing => &[][..],
        Printed::Recognised(songs) => songs.as_slice(),
    };
    catalog.keep_agreement(id, agreement(heard, &named))?;
    Ok(recognised)
}

fn named_on_the_phone(id: u64, track: &Track) -> Result<TrackToAsk> {
    let said = |text: &str| Some(text.trim().to_owned()).filter(|text| !text.is_empty());
    let tagged_title = said(&track.title).filter(|title| title != &track.name);
    let artist = said(&track.artist);
    let mbid = |id: &Option<String>| id.as_deref().and_then(|id| Mbid::new(id).ok());

    Ok(TrackToAsk {
        id: TrackId::new(id).map_err(|_| Error::UnknownTrack)?,
        title: track.title.clone(),
        tagged_title,
        tagged_artist: artist.clone(),
        artist,
        artist_mbid: mbid(&track.artist_mbid),
        album: None,
        album_title: None,
        album_answered: false,
        owner: said(&track.album_artist),
        owner_mbid: None,
        location: location(&track.uri)?,
        span: None,
        length: Some(Duration::from_millis(track.duration_ms)),
        mbid: mbid(&track.recording),
        isrc: None,
    })
}

#[uniffi::export]
impl MobilePlayer {
    pub fn select_output(&self, device: i32) -> Result<()> {
        if device < 0 || !self.routing.select(device) {
            return Err(Error::OutputUnavailable);
        }
        self.command(resonate_engine::Command::SetSink(None))
    }

    pub fn output_device(&self) -> i32 {
        self.routing.actual()
    }

    pub fn output_route_changed(&self, rate: u32, latency_ms: f64, rebind: bool) -> Result<()> {
        if self.routing.route_changed(rate, latency_ms) || rebind {
            self.command(resonate_engine::Command::SetSink(None))?;
        }
        Ok(())
    }

    pub fn configure_online(&self, enabled: bool) {
        self.services.online.client().reach(enabled);
    }

    pub fn filtered_tracks(
        &self,
        filter: TrackFilter,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Track>> {
        self.services
            .searching
            .filtered(&self.catalog, &filter, offset, limit)
    }

    pub fn statistics(&self, days: u32) -> Result<LibraryStatistics> {
        self.catalog.statistics(days)
    }

    pub fn listening_detail(
        &self,
        days: u32,
        limit: u32,
        time_zone: String,
        utc_offset_seconds: i32,
    ) -> Result<ListeningDetail> {
        self.catalog.listening_detail(
            days,
            limit,
            &the_listeners_calendar(&time_zone, utc_offset_seconds),
        )
    }

    pub fn library_breakdown(&self) -> Result<LibraryBreakdown> {
        self.catalog.library_breakdown()
    }

    pub fn artwork(&self, id: u64) -> Result<Option<Vec<u8>>> {
        self.catalog.pictured(id)
    }

    pub fn artwork_picture(&self, id: u64) -> Result<Option<u64>> {
        Ok(self.catalog.picture_of(id)?.map(|picture| picture as u64))
    }

    pub fn picture(&self, picture: u64) -> Result<Option<Vec<u8>>> {
        self.catalog.picture(picture)
    }

    pub fn set_artwork(&self, id: u64, bytes: Vec<u8>) -> Result<()> {
        self.keep_folder_cover(vec![id], bytes)
    }

    pub fn keep_folder_cover(&self, ids: Vec<u64>, bytes: Vec<u8>) -> Result<()> {
        if bytes.len() > 8 * 1024 * 1024 || ImageFormat::sniff(&bytes).is_none() {
            return Err(Error::InvalidPicture);
        }
        self.catalog.keep_artwork_of(&ids, &bytes, "folder", false)
    }

    pub fn inspector(&self, id: u64) -> Result<Inspector> {
        let track = self.catalog.track(id)?;
        let info = probe(&self.sources, &location(&track.uri)?)?;
        Ok(Inspector {
            track,
            container: info.container.to_string(),
            sample_format: info.spec.format.to_string(),
            seekable: info.is_seekable,
            encoder: info.tags.encoder.unwrap_or_default(),
            copyright: info.tags.copyright.unwrap_or_default(),
            comment: info.tags.comment.unwrap_or_default(),
            isrc: info.tags.isrc.unwrap_or_default(),
        })
    }

    pub fn analyse_track(&self, id: u64) -> Result<AnalysisReport> {
        let track = self.catalog.track(id)?;
        let watch = Arc::new(Watch::default());
        if let Some(previous) = self.services.analysis.lock().replace(Arc::clone(&watch)) {
            previous.stop();
        }
        let cached = self.services.analyses.lock().get(&id).cloned();
        let analysis = match cached {
            Some(analysis) => analysis,
            None => {
                let analysis = Arc::new(resonate_analysis::analyse(
                    &self.sources,
                    &location(&track.uri)?,
                    None,
                    watch.as_ref(),
                )?);
                let mut held = self.services.analyses.lock();
                if held.len() >= 8 {
                    held.pop_first();
                }
                held.insert(id, Arc::clone(&analysis));
                analysis
            }
        };
        let study = &analysis.study;
        let report = AnalysisReport {
            track_id: id,
            verdict: study.judgement.verdict.as_str().to_owned(),
            peak: study.levels.peak,
            rms: study.levels.rms,
            true_peak: study.loudness.true_peak,
            loudness_lufs: study.loudness.integrated,
            dynamic_range: study.loudness.dynamic_range.map(u32::from),
            clipped: study.levels.clipped,
            bits_in_use: study.levels.bits_in_use.map(u32::from),
            waveform: analysis
                .envelope
                .condensed(0, 512)
                .iter()
                .map(|reach| reach.high.abs().max(reach.low.abs()))
                .collect(),
            spectrum: study.spectrum.levels().to_vec(),
            spectrum_step_hz: study.spectrum.bin_hz(),
            findings: study.judgement.findings.iter().map(Finding::told).collect(),
        };
        self.catalog
            .keep_study(id, &report, &BandLevels::of(&study.spectrum))?;
        Ok(report)
    }

    pub fn cancel_analysis(&self) {
        if let Some(watch) = self.services.analysis.lock().as_ref() {
            watch.stop();
        }
    }

    pub fn analysis_progress(&self) -> Option<f32> {
        self.services
            .analysis
            .lock()
            .as_ref()
            .and_then(|watch| watch.share())
    }

    pub fn visualizer(&self, enabled: bool) -> Result<Vec<f32>> {
        let held = self.player.lock();
        let player = held.as_ref().ok_or(Error::Closed)?;
        player.listen_in(enabled);
        let mut left = vec![0.0; 256];
        if enabled && let Tapped::Samples(tap) = player.tap() {
            let mut right = vec![0.0; 256];
            tap.around(Instant::now(), &mut left, &mut right);
            for (left, right) in left.iter_mut().zip(right) {
                *left = (*left + right) / 2.0;
            }
        }
        Ok(left)
    }

    pub fn search_online(&self, text: String) -> Result<Vec<SearchSong>> {
        self.require_online()?;
        if let Some(link) = SongLink::read(&text) {
            if let SongLink::MusicBrainz(id) = &link {
                return self
                    .services
                    .online
                    .recording(id)
                    .map_err(|source| Error::reference(LookupOp::Recording, source))
                    .map(|song| {
                        song.into_iter()
                            .map(|song| search_song(song.into(), &[]))
                            .collect()
                    })
                    .and_then(|songs| self.offered(songs, Listing::Searched));
            }
            let Some(named) = self
                .services
                .online
                .song_linked(&link)
                .map_err(|source| Error::reference(LookupOp::FollowLink, source))?
            else {
                return Ok(Vec::new());
            };
            for isrc in &named.isrcs {
                let recordings = self
                    .services
                    .online
                    .recordings_of_isrc(isrc)
                    .map_err(|source| Error::reference(LookupOp::Isrc, source))?;
                if !recordings.is_empty() {
                    let songs = recordings
                        .into_iter()
                        .map(|song| search_song(song.into(), &[]))
                        .collect();
                    return self.offered(songs, Listing::Searched);
                }
            }
            let Some(title) = named.title else {
                return Ok(Vec::new());
            };
            return self
                .services
                .online
                .find_recording(&RecordingAsked {
                    title,
                    artist: named.artist,
                    artist_mbid: None,
                    release: None,
                    length: named.length,
                    wording: Wording::Phrase,
                })
                .map_err(|source| Error::reference(LookupOp::FindRecording, source))
                .map(|songs| {
                    songs
                        .into_iter()
                        .take(50)
                        .map(|song| search_song(song, &[]))
                        .collect()
                })
                .and_then(|songs| self.offered(songs, Listing::Searched));
        }
        let Some(asked) = resonate_library::songs_asked(&text) else {
            return Ok(Vec::new());
        };
        let typed = words_of_a_name(&asked.words);
        self.services
            .online
            .find_songs(&asked)
            .map_err(|source| Error::reference(LookupOp::FindRecording, source))
            .map(|songs| {
                songs
                    .into_iter()
                    .take(50)
                    .map(|song| {
                        let album = album_words_asked(&typed, &song);
                        search_song(song, &album)
                    })
                    .collect()
            })
            .and_then(|songs| self.offered(songs, Listing::Searched))
    }

    pub fn recognise_track(
        &self,
        id: u64,
        acoustid_key: String,
    ) -> Result<Vec<RecognitionCandidate>> {
        self.require_online()?;
        if !self.services.analyses.lock().contains_key(&id) {
            self.analyse_track(id)?;
        }
        let track = self.catalog.track(id)?;
        let analysis = self.services.analyses.lock().get(&id).cloned();
        let Some(print) = analysis.and_then(|analysis| analysis.study.print.clone()) else {
            return Ok(Vec::new());
        };
        let printer: Box<dyn Fingerprints> = if acoustid_key.trim().is_empty() {
            Box::new(resonate_online::ByEar::new(
                Arc::clone(self.services.online.client()),
                Arc::clone(&self.sources),
                Arc::new(std::sync::atomic::AtomicBool::new(true)),
            ))
        } else {
            Box::new(resonate_online::AcoustId::new(
                Arc::clone(self.services.online.client()),
                acoustid_key,
            ))
        };
        let recognised = recognised_through(&self.catalog, printer.as_ref(), id, track, print)?;
        Ok(match recognised {
            Printed::Nothing => Vec::new(),
            Printed::Recognised(songs) => songs
                .into_iter()
                .take(20)
                .map(|song| RecognitionCandidate {
                    score: u32::from(song.score),
                    song: search_song(song, &[]),
                })
                .collect(),
        })
    }

    pub fn enrich_track(&self, id: u64) -> Result<Track> {
        self.require_online()?;
        self.enrich_one(&Asking::over(Arc::clone(self.services.online.client())), id)?;
        self.catalog.track(id)
    }

    pub fn artist_information(&self, id: u64) -> Result<Option<ArtistInformation>> {
        let track = self.catalog.track(id)?;
        let Some(artist) = track
            .artist_mbid
            .as_deref()
            .and_then(|id| Mbid::new(id).ok())
        else {
            return Ok(None);
        };
        if let Some(kept) = self.catalog.kept_artist(artist.as_str())? {
            return Ok(Some(kept));
        }
        self.require_online()?;
        let Some(found) = self.online_artist(artist.to_string())? else {
            return Ok(None);
        };
        self.catalog
            .keep_artist(artist.as_str(), &found, SystemTime::now())?;
        Ok(Some(found))
    }

    pub fn online_artist(&self, id: String) -> Result<Option<ArtistInformation>> {
        self.require_online()?;
        let id = Mbid::new(&id)?;
        let Some(artist) = self
            .services
            .online
            .artist(&id)
            .map_err(|source| Error::reference(LookupOp::Artist, source))?
        else {
            return Ok(None);
        };
        let picture = match self.services.online.portrait(&artist.links) {
            Ok(picture) => picture.map(|art| art.bytes),
            Err(error) => {
                tracing::debug!(%error, "the artist portrait could not be fetched");
                None
            }
        };
        Ok(Some(ArtistInformation {
            name: artist.name,
            kind: artist.kind.unwrap_or_default(),
            country: artist.country.unwrap_or_default(),
            area: artist.area.unwrap_or_default(),
            genres: artist
                .genres
                .iter()
                .map(|genre| genre.name.clone())
                .collect(),
            links: artist.links.iter().map(|link| link.url.clone()).collect(),
            picture,
        }))
    }
}

impl MobilePlayer {
    pub(crate) fn require_online(&self) -> Result<()> {
        if self.services.online.client().is_reaching() {
            Ok(())
        } else {
            Err(Error::OnlineOff)
        }
    }

    pub(crate) fn count_listening(
        &self,
        state: &resonate_engine::PlayerState,
        queue: &resonate_engine::QueueRows,
    ) -> Result<()> {
        let mut listening = self.services.listening.lock();
        let counted = listening.0.heard(state, queue);
        self.store_counting(&mut listening.1, counted)
    }

    pub(crate) fn finish_listening(&self) -> Result<()> {
        let mut listening = self.services.listening.lock();
        let counted = listening.0.leaves();
        self.store_counting(&mut listening.1, counted)
    }

    fn store_counting(&self, visit: &mut Option<i64>, counted: Option<Counting>) -> Result<()> {
        if counted.is_none() {
            return Ok(());
        }
        let connection = self.catalog.0.lock();
        match counted {
            Some(Counting::Counts(played)) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    .min(i64::MAX as u64) as i64;
                let inserted = connection.execute("INSERT INTO listens(track, began, heard_ms, counted) SELECT id, ?2, ?3, 1 FROM documents WHERE uri = ?1", params![crate::documents::document_uri(&played.location)?, now.saturating_sub(played.heard.as_secs() as i64), milliseconds(played.heard).min(i64::MAX as u64) as i64]).map_err(|source| Error::store(StoreOp::Write, source))?;
                *visit = (inserted > 0).then(|| connection.last_insert_rowid());
            }
            Some(Counting::Hears(played) | Counting::Settles(played)) => {
                if let Some(id) = visit {
                    connection
                        .execute(
                            "UPDATE listens SET heard_ms = max(heard_ms, ?2) WHERE id = ?1",
                            params![*id, milliseconds(played.heard).min(i64::MAX as u64) as i64],
                        )
                        .map_err(|source| Error::store(StoreOp::Write, source))?;
                }
            }
            Some(Counting::Passes(heard)) => {
                connection.execute("INSERT INTO listens(track, began, heard_ms, counted) VALUES (NULL, unixepoch(), ?1, 0)", [milliseconds(heard).min(i64::MAX as u64) as i64]).map_err(|source| Error::store(StoreOp::Write, source))?;
            }
            None => {}
        }
        Ok(())
    }
}

pub(crate) fn milliseconds(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

pub(crate) fn search_song(song: RecordingMatch, album_asked: &[String]) -> SearchSong {
    let release = release_meant(&song.releases, album_asked).cloned();
    SearchSong {
        artist: song.credited_as(),
        artist_id: song
            .credit
            .first()
            .and_then(|credit| credit.mbid.as_ref())
            .map_or_else(String::new, ToString::to_string),
        recording: song.recording.to_string(),
        title: song.title,
        album: release
            .as_ref()
            .map_or_else(String::new, |release| release.title.clone()),
        release: release
            .as_ref()
            .map_or_else(String::new, |release| release.id.to_string()),
        release_group: String::new(),
        disc: release.as_ref().and_then(|release| release.disc),
        position: release.as_ref().and_then(|release| release.position),
        duration_ms: song.length.map(milliseconds),
        isrcs: song.isrcs.iter().map(ToString::to_string).collect(),
        held: false,
    }
}

fn release_meant<'a>(
    releases: &'a [RecordingRelease],
    album_asked: &[String],
) -> Option<&'a RecordingRelease> {
    let offered = in_the_order_worth_offering(releases);
    let asked_for = |release: &&&RecordingRelease| {
        let named = words_of_a_name(&release.title);
        !album_asked.is_empty() && album_asked.iter().all(|word| named.contains(word))
    };
    offered
        .iter()
        .find(asked_for)
        .or_else(|| offered.first())
        .copied()
}

fn album_words_asked(typed: &[String], song: &RecordingMatch) -> Vec<String> {
    let named: Vec<String> = words_of_a_name(&song.title)
        .into_iter()
        .chain(words_of_a_name(&song.credited_as()))
        .collect();
    typed
        .iter()
        .filter(|word| !named.contains(word))
        .cloned()
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Listing {
    Searched,
    Listed,
}

pub(crate) fn distinct_takes(songs: Vec<SearchSong>) -> Vec<SearchSong> {
    let mut kept: Vec<SearchSong> = Vec::with_capacity(songs.len());
    for song in songs {
        match kept.iter_mut().find(|held| same_take(held, &song)) {
            Some(held) if identity_held(&song) > identity_held(held) => *held = song,
            Some(_) => {}
            None => kept.push(song),
        }
    }
    kept
}

fn same_take(one: &SearchSong, other: &SearchSong) -> bool {
    let alike_within = milliseconds(A_LISTING_NAMED_ALIKE_MAY_DIFFER_BY);
    let lengths_agree = match (one.duration_ms, other.duration_ms) {
        (Some(one), Some(other)) => one.abs_diff(other) <= alike_within,
        (None, None) => true,
        _ => false,
    };
    lengths_agree
        && folded_letters(&one.title) == folded_letters(&other.title)
        && folded_letters(&one.artist) == folded_letters(&other.artist)
}

fn identity_held(song: &SearchSong) -> (usize, usize) {
    let known = [
        !song.recording.is_empty(),
        !song.isrcs.is_empty(),
        !song.release.is_empty(),
        !song.release_group.is_empty(),
        !song.artist_id.is_empty(),
        song.disc.is_some(),
        song.position.is_some(),
    ];
    (
        known.iter().filter(|known| **known).count(),
        song.isrcs.len(),
    )
}

#[cfg(test)]
mod tests {
    use resonate_library::{Agreement, Credit, Issued};

    use super::*;

    fn filed(title: &str, artist: &str) -> Track {
        Track {
            id: 1,
            uri: "content://music/a.flac".into(),
            name: "a.flac".into(),
            title: title.into(),
            artist: artist.into(),
            duration_ms: 305_000,
            ..Track::default()
        }
    }

    fn pressed(id: &str, title: &str, kind: &str, position: u32) -> RecordingRelease {
        RecordingRelease {
            id: Mbid::new(id).expect("an mbid"),
            title: title.to_owned(),
            date: None,
            disc: Some(1),
            position: Some(position),
            issued: Issued {
                kind: Some(kind.to_owned()),
                secondary: Vec::new(),
                status: Some("Official".to_owned()),
            },
        }
    }

    #[test]
    fn a_song_is_misnamed_only_where_audio_heard_as_another_song_meets_a_name_the_file_gave() {
        let heard = [joga()];

        let same = agreement(
            &heard,
            &named_on_the_phone(1, &filed("Jóga", "Björk")).unwrap(),
        );
        let other = agreement(
            &heard,
            &named_on_the_phone(1, &filed("Army of Me", "Björk")).unwrap(),
        );
        let by_another = agreement(
            &heard,
            &named_on_the_phone(1, &filed("Jóga", "Air")).unwrap(),
        );
        let untagged = agreement(
            &heard,
            &named_on_the_phone(1, &filed("a.flac", "")).unwrap(),
        );
        let unheard = agreement(
            &[],
            &named_on_the_phone(1, &filed("Jóga", "Björk")).unwrap(),
        );

        assert_eq!(same, Agreement::Agrees);
        assert_eq!(other, Agreement::Disagrees);
        assert_eq!(by_another, Agreement::Disagrees);
        assert_eq!(untagged, Agreement::Unnamed);
        assert_eq!(unheard, Agreement::Unheard);
    }

    fn joga() -> RecordingMatch {
        RecordingMatch {
            recording: Mbid::new("1b5b8f5d-6c3f-4f8b-8d3e-2d1f0a6b9c11").expect("an mbid"),
            score: 100,
            title: "Jóga".to_owned(),
            credit: vec![Credit {
                name: "Björk".to_owned(),
                joined_by: String::new(),
                mbid: Some(Mbid::new("87c5dedd-371d-4a53-9f7f-80522fb7f3cb").expect("an mbid")),
            }],
            length: Some(Duration::from_secs(305)),
            isrcs: Vec::new(),
            releases: vec![
                pressed("2a1c4e6b-1111-4c8f-9d2e-3b4a5c6d7e8f", "Jóga", "Single", 1),
                pressed(
                    "3b2d5f7c-2222-4d9a-8e3f-4c5b6d7e8f9a",
                    "Greatest Hits",
                    "Album",
                    7,
                ),
                pressed(
                    "4c3e6a8d-3333-4eab-9f4a-5d6c7e8f9a0b",
                    "Homogenic",
                    "Album",
                    2,
                ),
            ],
        }
    }

    #[test]
    fn a_found_song_is_placed_on_the_album_asked_for_else_on_the_one_worth_offering() {
        let typed = words_of_a_name("joga bjork homogenic");

        let asked = search_song(joga(), &album_words_asked(&typed, &joga()));
        let unasked = search_song(joga(), &[]);

        assert_eq!(asked.album, "Homogenic");
        assert_eq!(asked.release, "4c3e6a8d-3333-4eab-9f4a-5d6c7e8f9a0b");
        assert_eq!((asked.disc, asked.position), (Some(1), Some(2)));
        assert_eq!(asked.artist_id, "87c5dedd-371d-4a53-9f7f-80522fb7f3cb");
        assert_ne!(unasked.album, "Jóga");
    }

    mod counting {
        use resonate_core::{ChannelLayout, Frames, SampleFormat, SampleRate, StreamSpec, TrackId};
        use resonate_engine::{PlaybackState, PlayerState, QueueRows, TrackState};

        use crate::harness::Harness;

        const RATE: SampleRate = SampleRate::HZ_44100;

        fn seconds(count: f64) -> Frames {
            Frames((f64::from(RATE.hz()) * count) as u64)
        }

        fn rows(harness: &Harness) -> QueueRows {
            harness
                .player
                .player
                .lock()
                .as_ref()
                .expect("an open player")
                .queued()
                .rows
        }

        fn at(rows: &QueueRows, row: usize, position: f64, whole: Option<f64>) -> PlayerState {
            let id: TrackId = rows.get(row).expect("a queue row").id;

            PlayerState {
                playback: PlaybackState::Playing,
                current: Some(TrackState {
                    id,
                    source: StreamSpec::new(RATE, ChannelLayout::Stereo, SampleFormat::S16),
                    position: seconds(position),
                    duration: whole.map(seconds),
                }),
                queue_position: Some(row),
                ..PlayerState::default()
            }
        }

        fn heard(harness: &Harness) -> Vec<(Option<i64>, i64, i64)> {
            let connection = harness.player.catalog.0.lock();
            let mut query = connection
                .prepare("SELECT track, heard_ms, counted FROM listens ORDER BY id")
                .expect("a query");
            query
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
                .expect("listens")
                .collect::<rusqlite::Result<Vec<_>>>()
                .expect("rows")
        }

        fn walked(
            harness: &Harness,
            rows: &QueueRows,
            row: usize,
            positions: &[f64],
            whole: Option<f64>,
        ) {
            for position in positions {
                harness
                    .player
                    .count_listening(&at(rows, row, *position, whole), rows)
                    .expect("a listen counted");
            }
        }

        #[test]
        fn half_a_song_heard_is_one_counted_listen_and_what_is_heard_after_it_settles_on_leaving() {
            let harness = Harness::new();
            harness.resumed("ab", 0, 0);
            let rows = rows(&harness);
            let song = harness.song('a').id as i64;

            walked(&harness, &rows, 0, &[0.0, 0.5, 1.0], Some(2.0));
            assert_eq!(heard(&harness), [(Some(song), 1_000, 1)]);

            walked(&harness, &rows, 0, &[1.5, 2.0], Some(2.0));
            harness.player.finish_listening().expect("a listen settled");

            assert_eq!(heard(&harness), [(Some(song), 2_000, 1)]);
        }

        #[test]
        fn a_stretch_heard_without_being_counted_is_a_pass_naming_no_song() {
            let harness = Harness::new();
            harness.resumed("ab", 0, 0);
            let rows = rows(&harness);

            walked(&harness, &rows, 0, &[0.0, 1.0, 2.0, 3.0], None);
            harness.player.finish_listening().expect("a pass stored");

            assert_eq!(heard(&harness), [(None, 3_000, 0)]);
        }

        #[test]
        fn a_listen_is_told_again_as_it_grows_and_never_makes_a_second_row() {
            let harness = Harness::new();
            harness.resumed("ab", 0, 0);
            let rows = rows(&harness);
            let song = harness.song('a').id as i64;
            let positions: Vec<f64> = (0..=300).step_by(4).map(f64::from).collect();

            walked(&harness, &rows, 0, &positions, None);

            let stored = heard(&harness);
            assert_eq!(stored.len(), 1, "{stored:?}");
            assert_eq!(stored[0].0, Some(song));
            assert_eq!(stored[0].2, 1);
            assert!(stored[0].1 >= 270_000, "{stored:?}");
        }

        #[test]
        fn a_listen_counted_for_a_song_the_catalog_lost_does_not_settle_over_another_listen() {
            let harness = Harness::new();
            harness.resumed("ab", 0, 0);
            let rows = rows(&harness);
            let (kept, lost) = (harness.song('a').id as i64, harness.song('b').id);

            walked(&harness, &rows, 0, &[0.0, 0.5, 1.0], Some(2.0));
            harness.player.finish_listening().expect("a listen settled");
            assert_eq!(heard(&harness), [(Some(kept), 1_000, 1)]);

            harness
                .player
                .catalog
                .0
                .lock()
                .execute("DELETE FROM documents WHERE id = ?1", [lost as i64])
                .expect("a song forgotten");
            walked(&harness, &rows, 1, &[0.0, 0.5, 1.0], Some(2.0));
            walked(&harness, &rows, 1, &[1.5, 2.0], Some(2.0));
            harness.player.finish_listening().expect("a listen settled");

            assert_eq!(heard(&harness), [(Some(kept), 1_000, 1)]);
        }
    }

    mod reports {
        use std::{fs, thread, time::Instant};

        use crate::harness::{Harness, LOOKED_AGAIN_AFTER, SETTLED_WITHIN, silence};

        fn with_info(wave: &[u8], entries: &[(&[u8; 4], &str)]) -> Vec<u8> {
            let mut list = b"INFO".to_vec();
            for (id, text) in entries {
                let mut value = text.as_bytes().to_vec();
                value.push(0);
                list.extend_from_slice(*id);
                list.extend_from_slice(&(value.len() as u32).to_le_bytes());
                list.extend_from_slice(&value);
                if value.len() % 2 == 1 {
                    list.push(0);
                }
            }
            let mut chunk = b"LIST".to_vec();
            chunk.extend_from_slice(&(list.len() as u32).to_le_bytes());
            chunk.extend_from_slice(&list);

            let data_at = wave
                .windows(4)
                .position(|window| window == b"data")
                .expect("a data chunk");
            let mut whole = wave[..data_at].to_vec();
            whole.extend_from_slice(&chunk);
            whole.extend_from_slice(&wave[data_at..]);
            let riff = (whole.len() - 8) as u32;
            whole[4..8].copy_from_slice(&riff.to_le_bytes());
            whole
        }

        #[test]
        fn the_inspector_reads_the_container_the_sample_word_and_the_tags_the_file_declares() {
            let harness = Harness::new();
            let song = harness.song('a').clone();
            fs::write(
                harness.folder.join("a.wav"),
                with_info(
                    &silence(),
                    &[
                        (b"ISFT", "Exact Audio Copy"),
                        (b"ICOP", "(c) 1971 Harvest"),
                        (b"ICMT", "ripped from the 1994 remaster"),
                    ],
                ),
            )
            .expect("a tagged song");

            let inspected = harness.player.inspector(song.id).expect("an inspector");

            assert_eq!(inspected.track.id, song.id);
            assert!(!inspected.container.is_empty());
            assert!(!inspected.sample_format.is_empty());
            assert!(inspected.seekable);
            assert_eq!(inspected.encoder, "Exact Audio Copy");
            assert_eq!(inspected.copyright, "(c) 1971 Harvest");
            assert_eq!(inspected.comment, "ripped from the 1994 remaster");
            assert_eq!(inspected.isrc, "");
        }

        #[test]
        fn a_file_declaring_nothing_inspects_with_blank_tags_and_a_song_the_catalog_lacks_is_refused()
         {
            let harness = Harness::new();
            let song = harness.song('b').id;

            let inspected = harness.player.inspector(song).expect("an inspector");

            assert_eq!(
                (
                    inspected.encoder,
                    inspected.copyright,
                    inspected.comment,
                    inspected.isrc
                ),
                (String::new(), String::new(), String::new(), String::new())
            );
            assert!(harness.player.inspector(song + 1_000).is_err());
        }

        #[test]
        fn an_analysis_reports_the_levels_and_is_served_again_from_memory_once_the_file_is_gone() {
            let harness = Harness::new();
            let song = harness.song('c').id;

            let report = harness.player.analyse_track(song).expect("a report");

            assert_eq!(report.track_id, song);
            assert!(!report.verdict.is_empty());
            assert_eq!(report.peak, 0.0);
            assert_eq!(report.rms, 0.0);
            assert_eq!(report.loudness_lufs, None);
            assert!(report.waveform.len() <= 512);
            assert!(report.waveform.iter().all(|height| *height == 0.0));

            fs::remove_file(harness.folder.join("c.wav")).expect("a file gone");
            let again = harness.player.analyse_track(song).expect("a cached report");

            assert_eq!(again.verdict, report.verdict);
            assert_eq!(harness.player.services.analyses.lock().len(), 1);
            let gone = harness.song('d').id;
            fs::remove_file(harness.folder.join("d.wav")).expect("a file gone");
            assert!(harness.player.analyse_track(gone).is_err());
            assert!(harness.player.analyse_track(song + 1_000).is_err());
        }

        #[test]
        fn a_failure_the_engine_reports_is_told_by_one_snapshot_and_by_no_other() {
            let harness = Harness::new();
            let ids = harness.ids("ab");
            fs::remove_file(harness.folder.join("a.wav")).expect("a file gone");

            harness.player.load(ids, 0, false).expect("a queue loaded");
            let began = Instant::now();
            let told = loop {
                if let Some(told) = harness.player.snapshot().expect("a snapshot").failure {
                    break told;
                }
                assert!(began.elapsed() < SETTLED_WITHIN, "no failure was told");
                thread::sleep(LOOKED_AGAIN_AFTER);
            };

            assert!(!told.is_empty());
            assert_eq!(harness.player.snapshot().expect("a snapshot").failure, None);
        }
    }
}
