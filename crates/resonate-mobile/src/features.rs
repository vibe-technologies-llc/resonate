use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use resonate_analysis::Watch;
use resonate_codec::{ImageFormat, probe};
use resonate_core::{Mbid, SourceId};
use resonate_engine::{Counting, Listening, Tapped};
use resonate_library::{
    Fingerprints, LookupOp, LyricsAsked, Printed, RecordingAsked, RecordingMatch, Reference,
    SongLink, Sounded, Wording,
};
use resonate_lyrics::{Lyrics, read_lyrics, read_lyricsfile};
use resonate_online::{Identity, Online};
use rusqlite::params;

use crate::{Error, MobilePlayer, Result, StoreOp, Track, documents::location, enrichment::Asking};

pub(crate) struct Services {
    pub(crate) online: Online,
    pub(crate) auto_eq: resonate_online::AutoEq,
    lyrics: Mutex<BTreeMap<u64, Arc<Lyrics>>>,
    analysis: Mutex<Option<Arc<Watch>>>,
    pub(crate) analyses: Mutex<BTreeMap<u64, Arc<resonate_analysis::Analysis>>>,
    listening: Mutex<(Listening, Option<i64>)>,
}

impl Default for Services {
    fn default() -> Self {
        let online = Online::new(Identity::of_this_build());
        online.client().reach(false);
        let auto_eq = resonate_online::AutoEq::new(Arc::clone(online.client()), None);
        Self {
            online,
            auto_eq,
            lyrics: Mutex::new(BTreeMap::new()),
            analysis: Mutex::new(None),
            analyses: Mutex::new(BTreeMap::new()),
            listening: Mutex::new((Listening::default(), None)),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, uniffi::Enum)]
pub enum TrackOrder {
    #[default]
    Title,
    Album,
    Artist,
    Duration,
    Added,
    Plays,
}

#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct TrackFilter {
    pub text: String,
    pub genre: String,
    pub artist: String,
    pub album: String,
    pub minimum_rate: u32,
    pub minimum_bits: u32,
    pub favourites: bool,
    pub unplayed: bool,
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

#[derive(Clone, Debug, uniffi::Record)]
pub struct LyricWord {
    pub text: String,
    pub at_ms: u64,
    pub until_ms: Option<u64>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct LyricRow {
    pub text: String,
    pub at_ms: Option<u64>,
    pub until_ms: Option<u64>,
    pub voice: u32,
    pub words: Vec<LyricWord>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct LyricSheet {
    pub track_id: u64,
    pub origin: String,
    pub lines: Vec<LyricRow>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct LyricFrame {
    pub line: Option<u32>,
    pub active: Vec<u32>,
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

#[derive(Clone, Debug, uniffi::Record)]
pub struct SearchSong {
    pub recording: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: Option<u64>,
    pub isrcs: Vec<String>,
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
        self.catalog.filtered(&filter, offset, limit)
    }

    pub fn statistics(&self, days: u32) -> Result<LibraryStatistics> {
        self.catalog.statistics(days)
    }

    pub fn artwork(&self, id: u64) -> Result<Option<Vec<u8>>> {
        self.catalog.pictured(id)
    }

    pub fn set_artwork(&self, id: u64, bytes: Vec<u8>) -> Result<()> {
        if bytes.len() > 8 * 1024 * 1024 || ImageFormat::sniff(&bytes).is_none() {
            return Err(Error::InvalidPicture);
        }
        self.catalog.keep_artwork(id, &bytes, "folder", false)
    }

    pub fn inspector(&self, id: u64) -> Result<Inspector> {
        let track = self.catalog.track(id)?;
        let info = probe(&self.sources, &location(&track.uri)?)?;
        Ok(Inspector {
            track,
            container: format!("{:?}", info.container),
            sample_format: info.spec.format.to_string(),
            seekable: info.is_seekable,
            encoder: info.tags.encoder.unwrap_or_default(),
            copyright: info.tags.copyright.unwrap_or_default(),
            comment: info.tags.comment.unwrap_or_default(),
            isrc: info.tags.isrc.unwrap_or_default(),
        })
    }

    pub fn keep_lyric_text(&self, id: u64, text: String) -> Result<()> {
        let parsed = read_lyrics(SourceId::new("embedded")?, &text)?;
        if let Some(parsed) = parsed {
            let previous = self.cached_lyrics(id)?;
            if previous
                .as_ref()
                .is_none_or(|held| parsed.detail() > held.detail())
            {
                self.catalog
                    .keep_lyrics(id, Some(&text), None, "embedded")?;
                self.remember_lyrics(id, parsed);
            }
        }
        Ok(())
    }

    pub fn keep_sidecar_lyrics(&self, id: u64, bytes: Vec<u8>) -> Result<()> {
        let track = self.catalog.track(id)?;
        let mut wanted = resonate_lyrics::Wanted::for_media(location(&track.uri)?);
        wanted.title = Some(track.title);
        wanted.artist = Some(track.artist);
        wanted.duration =
            (track.duration_ms > 0).then_some(Duration::from_millis(track.duration_ms));
        if let Some(lyrics) =
            resonate_lyrics::read_sidecar(SourceId::new("sidecar")?, &bytes, &wanted)?
        {
            let previous = self.cached_lyrics(id)?;
            if previous
                .as_ref()
                .is_none_or(|held| lyrics.detail() >= held.detail())
            {
                self.catalog.keep_lyrics(
                    id,
                    Some(&resonate_core::text::decoded(&bytes).0),
                    None,
                    "sidecar",
                )?;
                self.remember_lyrics(id, lyrics);
            }
        }
        Ok(())
    }

    pub fn lyrics(&self, id: u64, refresh: bool) -> Result<LyricSheet> {
        let track = self.catalog.track(id)?;
        let cached = self.cached_lyrics(id)?;
        if self.services.online.client().is_reaching()
            && (refresh || self.catalog.lyric_text(id)?.is_none())
        {
            let answer = self
                .services
                .online
                .lyrics(&LyricsAsked {
                    title: track.title,
                    artist: track.artist,
                    album: (!track.album.is_empty()).then_some(track.album),
                    length: (track.duration_ms > 0)
                        .then_some(Duration::from_millis(track.duration_ms)),
                })
                .map_err(|source| Error::reference(LookupOp::Lyrics, source));
            match answer {
                Ok(Some(answer)) => {
                    let parsed =
                        parse_lyrics(&answer.text, answer.lyricsfile.as_deref(), "lrclib")?;
                    if let Some(parsed) = parsed
                        && cached
                            .as_ref()
                            .is_none_or(|held| parsed.detail() >= held.detail())
                    {
                        self.catalog.keep_lyrics(
                            id,
                            Some(&answer.text),
                            answer.lyricsfile.as_deref(),
                            "lrclib",
                        )?;
                        self.remember_lyrics(id, parsed);
                    }
                }
                Ok(None) if cached.is_none() => {
                    self.catalog.keep_lyrics(id, None, None, "lrclib")?
                }
                Ok(None) => {}
                Err(error) if cached.is_none() => return Err(error),
                Err(error) => tracing::debug!(%error, "cached lyrics cover a refused lookup"),
            }
        }
        let lyrics = self.cached_lyrics(id)?;
        Ok(LyricSheet {
            track_id: id,
            origin: lyrics
                .as_ref()
                .map_or_else(String::new, |lyrics| lyrics.source().to_string()),
            lines: lyrics.map_or_else(Vec::new, |lyrics| {
                lyrics
                    .lines()
                    .iter()
                    .map(|line| LyricRow {
                        text: line.text.clone(),
                        at_ms: line.at.map(milliseconds),
                        until_ms: line.until.map(milliseconds),
                        voice: line.voice.index() as u32,
                        words: line
                            .words
                            .iter()
                            .map(|word| LyricWord {
                                text: word.text.clone(),
                                at_ms: milliseconds(word.at),
                                until_ms: word.until.map(milliseconds),
                            })
                            .collect(),
                    })
                    .collect()
            }),
        })
    }

    pub fn lyric_frame(&self, id: u64, position_ms: u64) -> LyricFrame {
        let held = self.services.lyrics.lock();
        let lyrics = held.get(&id);
        let position = Duration::from_millis(position_ms);
        LyricFrame {
            line: lyrics
                .and_then(|lyrics| lyrics.line_at(position))
                .map(|line| line as u32),
            active: lyrics.map_or_else(Vec::new, |lyrics| {
                lyrics
                    .voices_in_play(position)
                    .into_iter()
                    .flatten()
                    .map(|line| line as u32)
                    .collect()
            }),
        }
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
            findings: study
                .judgement
                .findings
                .iter()
                .map(|finding| format!("{finding:?}"))
                .collect(),
        };
        self.catalog.keep_study(id, &report)?;
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
                            .map(RecordingMatch::from)
                            .map(search_song)
                            .collect()
                    });
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
                    return Ok(recordings
                        .into_iter()
                        .map(RecordingMatch::from)
                        .map(search_song)
                        .collect());
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
                .map(|songs| songs.into_iter().take(50).map(search_song).collect());
        }
        let Some(asked) = resonate_library::songs_asked(&text) else {
            return Ok(Vec::new());
        };
        self.services
            .online
            .find_songs(&asked)
            .map_err(|source| Error::reference(LookupOp::FindRecording, source))
            .map(|songs| songs.into_iter().take(50).map(search_song).collect())
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
        let sounded = Sounded {
            location: location(&track.uri)?,
            span: None,
            length: Some(Duration::from_millis(track.duration_ms)),
            title: Some(track.title),
            artist: Some(track.artist),
            print,
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
        let recognised = printer
            .recognise(&sounded)
            .map_err(|source| Error::reference(LookupOp::Recognise, source))?;
        Ok(match recognised {
            Printed::Nothing => Vec::new(),
            Printed::Recognised(songs) => songs
                .into_iter()
                .take(20)
                .map(|song| RecognitionCandidate {
                    score: u32::from(song.score),
                    song: search_song(song),
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

    fn cached_lyrics(&self, id: u64) -> Result<Option<Arc<Lyrics>>> {
        if let Some(lyrics) = self.services.lyrics.lock().get(&id).cloned() {
            return Ok(Some(lyrics));
        }
        if let Some((Some(text), document, origin)) = self.catalog.lyric_text(id)?
            && let Some(lyrics) = parse_lyrics(&text, document.as_deref(), &origin)?
        {
            self.remember_lyrics(id, lyrics);
        }
        Ok(self.services.lyrics.lock().get(&id).cloned())
    }

    fn remember_lyrics(&self, id: u64, lyrics: Lyrics) {
        let mut held = self.services.lyrics.lock();
        if held.len() >= 16 {
            held.pop_first();
        }
        held.insert(id, Arc::new(lyrics));
    }

    pub(crate) fn count_listening(
        &self,
        state: &resonate_engine::PlayerState,
        queue: &[resonate_engine::QueueItem],
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
        let connection = self.catalog.0.lock();
        match counted {
            Some(Counting::Counts(played)) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    .min(i64::MAX as u64) as i64;
                connection.execute("INSERT INTO listens(track, began, heard_ms, counted) SELECT id, ?2, ?3, 1 FROM documents WHERE uri = ?1", params![crate::documents::document_uri(&played.location)?, now.saturating_sub(played.heard.as_secs() as i64), milliseconds(played.heard).min(i64::MAX as u64) as i64]).map_err(|source| Error::store(StoreOp::Write, source))?;
                *visit = Some(connection.last_insert_rowid());
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

fn parse_lyrics(text: &str, document: Option<&str>, origin: &str) -> Result<Option<Lyrics>> {
    let source = SourceId::new(origin)?;
    if let Some(document) = document
        && let Ok(read) = read_lyricsfile(source.clone(), document)
        && read.lyrics.is_some()
    {
        return Ok(read.lyrics);
    }
    Ok(read_lyrics(source, text)?)
}

fn milliseconds(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

pub(crate) fn search_song(song: RecordingMatch) -> SearchSong {
    SearchSong {
        artist: song.credited_as(),
        recording: song.recording.to_string(),
        title: song.title,
        album: song
            .releases
            .first()
            .map_or_else(String::new, |release| release.title.clone()),
        duration_ms: song.length.map(milliseconds),
        isrcs: song.isrcs.iter().map(ToString::to_string).collect(),
    }
}
