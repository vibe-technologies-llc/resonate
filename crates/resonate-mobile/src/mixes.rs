use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use resonate_core::{Accent, Calendar};
use resonate_library::{
    Candidate, Filing, Kinship, OF_LATE, Reason, SavedQuery, SuggestionKind, Weighed,
    artists_as_heard, artists_heard_lately, genres_ranked_by, lists_to_offer, worth_offering,
};

use crate::{
    MobilePlayer, Result, Track,
    catalog::Catalog,
    catalog_extras::the_listeners_calendar,
    saved_searches::SavedSearch,
    search::{Index, LISTED_AT_MOST, Searching},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MixAccent {
    Mauve,
    Blue,
    Teal,
    Green,
    Amber,
    Peach,
    Red,
}

impl From<Accent> for MixAccent {
    fn from(accent: Accent) -> Self {
        match accent {
            Accent::Mauve => Self::Mauve,
            Accent::Blue => Self::Blue,
            Accent::Teal => Self::Teal,
            Accent::Green => Self::Green,
            Accent::Amber => Self::Amber,
            Accent::Peach => Self::Peach,
            Accent::Red => Self::Red,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MixAccents {
    pub from: MixAccent,
    pub to: MixAccent,
}

#[uniffi::export]
pub fn named_accents(name: String) -> MixAccents {
    let (from, to) = Accent::named_pair(&name);

    MixAccents {
        from: from.into(),
        to: to.into(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SuggestionShelf {
    ForYou,
    Rediscover,
    Listening,
    Eras,
    Genres,
    Artists,
    Sound,
}

impl From<SuggestionKind> for SuggestionShelf {
    fn from(kind: SuggestionKind) -> Self {
        match kind {
            SuggestionKind::ForYou => Self::ForYou,
            SuggestionKind::Rediscover => Self::Rediscover,
            SuggestionKind::Listening => Self::Listening,
            SuggestionKind::Eras => Self::Eras,
            SuggestionKind::Genres => Self::Genres,
            SuggestionKind::Artists => Self::Artists,
            SuggestionKind::Sound => Self::Sound,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SuggestionReason {
    Decade,
    GenreOfADecade,
    Genre,
    Artist,
    NeverHeard,
    MostPlayed,
    RecentlyAdded,
    FreshArrivals,
    BackCatalogue,
    OnRepeat,
    KindredArtists,
    DeepCuts,
    ArtistToDiscover,
    UnheardGenre,
    ForgottenFavourites,
    OldFlames,
    SecondChances,
    StarredUnheard,
    ArtistLeftBehind,
    HiRes,
    LongPlayers,
    SlowBurners,
    ShortAndSharp,
    Surround,
    Mono,
}

impl From<Reason> for SuggestionReason {
    fn from(reason: Reason) -> Self {
        match reason {
            Reason::Decade(_) => Self::Decade,
            Reason::GenreOfADecade(_) => Self::GenreOfADecade,
            Reason::Genre => Self::Genre,
            Reason::Artist => Self::Artist,
            Reason::NeverHeard => Self::NeverHeard,
            Reason::MostPlayed => Self::MostPlayed,
            Reason::RecentlyAdded => Self::RecentlyAdded,
            Reason::FreshArrivals => Self::FreshArrivals,
            Reason::BackCatalogue => Self::BackCatalogue,
            Reason::OnRepeat => Self::OnRepeat,
            Reason::KindredArtists => Self::KindredArtists,
            Reason::DeepCuts => Self::DeepCuts,
            Reason::ArtistToDiscover => Self::ArtistToDiscover,
            Reason::UnheardGenre => Self::UnheardGenre,
            Reason::ForgottenFavourites => Self::ForgottenFavourites,
            Reason::OldFlames => Self::OldFlames,
            Reason::SecondChances => Self::SecondChances,
            Reason::StarredUnheard => Self::StarredUnheard,
            Reason::ArtistLeftBehind => Self::ArtistLeftBehind,
            Reason::HiRes => Self::HiRes,
            Reason::LongPlayers => Self::LongPlayers,
            Reason::SlowBurners => Self::SlowBurners,
            Reason::ShortAndSharp => Self::ShortAndSharp,
            Reason::Surround => Self::Surround,
            Reason::Mono => Self::Mono,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SuggestedList {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub reason: SuggestionReason,
    pub shelf: SuggestionShelf,
    pub shelf_name: String,
    pub search: SavedSearch,
    pub songs: u32,
    pub duration_ms: u64,
    pub covers: Vec<u64>,
    pub accents: MixAccents,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MeasuredSearch {
    pub songs: u32,
    pub duration_ms: u64,
    pub covers: Vec<u64>,
}

pub(crate) struct Offered {
    candidate: Candidate,
    weighed: Weighed,
    covers: Vec<u64>,
}

pub(crate) struct KeptOffer {
    index: Arc<Index>,
    today: i64,
    offered: Arc<[Offered]>,
}

impl Offered {
    fn listed(&self) -> SuggestedList {
        let Candidate {
            name,
            reason,
            query,
            ..
        } = &self.candidate;
        let (from, to) = reason.accents(name);

        SuggestedList {
            id: id_of(query),
            name: name.clone(),
            detail: reason.says(),
            reason: (*reason).into(),
            shelf: reason.kind().into(),
            shelf_name: reason.kind().name().to_owned(),
            search: SavedSearch::of(query),
            songs: self.weighed.rows,
            duration_ms: self.weighed.length.map_or(0, |length| {
                u64::try_from(length.as_millis()).unwrap_or(u64::MAX)
            }),
            covers: self.covers.clone(),
            accents: MixAccents {
                from: from.into(),
                to: to.into(),
            },
        }
    }
}

fn id_of(query: &SavedQuery) -> String {
    format!(
        "{}.{}.{}.{}",
        query.sort.code(),
        query.reading.code(),
        query.limit.unwrap_or_default(),
        query.text.as_deref().unwrap_or_default()
    )
}

fn shelf_position(offered: &Offered) -> usize {
    let kind = offered.candidate.reason.kind();

    SuggestionKind::ALL
        .iter()
        .position(|held| *held == kind)
        .unwrap_or(SuggestionKind::ALL.len())
}

fn seconds_since_the_epoch(moment: SystemTime) -> i64 {
    moment.duration_since(UNIX_EPOCH).map_or(0, |since| {
        i64::try_from(since.as_secs()).unwrap_or(i64::MAX)
    })
}

pub(crate) fn offer(
    searching: &Searching,
    catalog: &Catalog,
    calendar: &Calendar,
    now: SystemTime,
) -> Result<Arc<[Offered]>> {
    let index = searching.index(catalog)?;
    let today = calendar.day_of(now);
    if let Some(kept) = searching.offering.lock().as_ref()
        && Arc::ptr_eq(&kept.index, &index)
        && kept.today == today
    {
        return Ok(Arc::clone(&kept.offered));
    }

    let heard = artists_as_heard(index.artists_heard());
    let of_late = now.checked_sub(OF_LATE).unwrap_or(UNIX_EPOCH);
    let lately =
        artists_heard_lately(searching.heard_lately(catalog, seconds_since_the_epoch(of_late))?);
    let filing = Filing::of(
        index.years_held(),
        index.genres_held(),
        index.genres_of_years(),
        Kinship::of(&index.genres_of_artists()),
    );
    let played_most = genres_ranked_by(index.genres_played());
    let candidates = lists_to_offer(&filing, &heard, &lately, &played_most, today, now);

    let mut pictured: HashMap<String, Vec<u64>> = HashMap::new();
    let mut offered: Vec<Offered> = worth_offering(candidates, |query| -> Result<Weighed> {
        let measured = searching.measured(catalog, query)?;
        pictured.insert(id_of(query), measured.covers);

        Ok(Weighed {
            rows: u32::try_from(measured.songs).unwrap_or(u32::MAX),
            length: (measured.length_ms > 0).then(|| Duration::from_millis(measured.length_ms)),
        })
    })?
    .into_iter()
    .map(|(candidate, weighed)| Offered {
        covers: pictured
            .remove(&id_of(&candidate.query))
            .unwrap_or_default(),
        candidate,
        weighed,
    })
    .collect();
    offered.sort_by_key(shelf_position);

    let offered: Arc<[Offered]> = offered.into();
    *searching.offering.lock() = Some(KeptOffer {
        index,
        today,
        offered: Arc::clone(&offered),
    });

    Ok(offered)
}

#[uniffi::export]
impl MobilePlayer {
    pub fn suggested_lists(
        &self,
        time_zone: String,
        utc_offset_seconds: i32,
    ) -> Result<Vec<SuggestedList>> {
        let calendar = the_listeners_calendar(&time_zone, utc_offset_seconds);

        Ok(offer(
            &self.services.searching,
            &self.catalog,
            &calendar,
            SystemTime::now(),
        )?
        .iter()
        .map(Offered::listed)
        .collect())
    }

    pub fn measure_saved_search(&self, search: SavedSearch) -> Result<MeasuredSearch> {
        let measured = self
            .services
            .searching
            .measured(&self.catalog, &search.query())?;

        Ok(MeasuredSearch {
            songs: u32::try_from(measured.songs).unwrap_or(u32::MAX),
            duration_ms: measured.length_ms,
            covers: measured.covers,
        })
    }

    pub fn saved_search_page(
        &self,
        search: SavedSearch,
        offset: u32,
        limit: u32,
    ) -> Result<Vec<Track>> {
        self.services.searching.saved_page(
            &self.catalog,
            &search.query(),
            offset as usize,
            limit.min(LISTED_AT_MOST) as usize,
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use rusqlite::params;

    use super::*;

    const A_DAY: Duration = Duration::from_secs(24 * 60 * 60);
    const A_WEEK: Duration = Duration::from_secs(7 * 24 * 60 * 60);
    const LONG_AGO: Duration = Duration::from_secs(250 * 24 * 60 * 60);

    fn shelve(
        catalog: &Catalog,
        artist: &str,
        genre: &str,
        songs: usize,
        listens: usize,
        heard_ago: Duration,
        now: SystemTime,
    ) {
        let began = seconds_since_the_epoch(now.checked_sub(heard_ago).expect("a moment"));
        for number in 0..songs {
            let held = catalog
                .upsert(Track {
                    uri: format!("content://test/{artist}/{number}-{listens}.flac"),
                    name: format!("{number}.flac"),
                    title: format!("{artist} {number}"),
                    artist: artist.into(),
                    album: artist.into(),
                    codec: "flac".into(),
                    rate: 44_100,
                    duration_ms: 200_000 + (artist.len() * 1_000 + number) as u64,
                    ..Track::default()
                })
                .expect("a song");
            let connection = catalog.0.lock();
            connection
                .execute(
                    "UPDATE documents SET genre = ?2 WHERE id = ?1",
                    params![held.id as i64, genre],
                )
                .expect("a genre");
            for _ in 0..listens {
                connection
                    .execute(
                        "INSERT INTO listens (track, began, heard_ms, counted) VALUES (?1, ?2, 180000, 1)",
                        params![held.id as i64, began],
                    )
                    .expect("a listen");
            }
        }
    }

    fn a_listeners_catalog(now: SystemTime) -> Catalog {
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        shelve(&catalog, "Played Lately", "Krautrock", 12, 3, A_WEEK, now);
        shelve(&catalog, "Played Lately", "Krautrock", 6, 0, A_WEEK, now);
        shelve(&catalog, "A Neighbour", "Krautrock", 7, 0, A_WEEK, now);
        shelve(&catalog, "Long Gone", "Polka", 12, 5, LONG_AGO, now);
        shelve(&catalog, "Faded Out", "Fado", 5, 6, LONG_AGO, now);
        for quiet in ["Quiet One", "Quiet Two", "Quiet Three"] {
            shelve(&catalog, quiet, "Ambient", 6, 0, A_WEEK, now);
        }

        catalog
    }

    fn names(offered: &[Offered]) -> Vec<&str> {
        offered
            .iter()
            .map(|held| held.candidate.name.as_str())
            .collect()
    }

    fn songs_of(offered: &[Offered], name: &str) -> u32 {
        offered
            .iter()
            .find(|held| held.candidate.name == name)
            .map_or_else(
                || panic!("{name} was not offered among {:?}", names(offered)),
                |held| held.weighed.rows,
            )
    }

    #[test]
    fn what_the_phone_heard_decides_the_lists_it_is_offered() {
        let now = SystemTime::now();
        let catalog = a_listeners_catalog(now);
        let searching = Searching::default();

        let offered = offer(&searching, &catalog, &Calendar::utc(), now).expect("lists");

        assert_eq!(songs_of(&offered, "On repeat"), 12);
        assert_eq!(songs_of(&offered, "Old flames"), 17);
        assert_eq!(songs_of(&offered, "Played Lately, deep cuts"), 6);
        assert_eq!(songs_of(&offered, "More like Played Lately"), 7);
        assert!(
            names(&offered)
                .iter()
                .any(|name| name.starts_with("Discover ")),
            "{:?}",
            names(&offered)
        );
        assert!(!names(&offered).contains(&"Forgotten favourites"));
    }

    #[test]
    fn a_list_holds_as_many_songs_when_it_is_opened_as_it_said() {
        let now = SystemTime::now();
        let catalog = a_listeners_catalog(now);
        let searching = Searching::default();

        let offered = offer(&searching, &catalog, &Calendar::utc(), now).expect("lists");

        assert!(!offered.is_empty());
        for held in offered.iter() {
            let opened = searching
                .saved_page(&catalog, &held.candidate.query, 0, 500)
                .expect("the list opens");
            assert_eq!(
                opened.len() as u32,
                held.weighed.rows.min(500),
                "{}",
                held.candidate.name
            );
        }
    }

    #[test]
    fn the_lists_come_in_the_desktops_shelves_made_for_you_first() {
        let now = SystemTime::now();
        let catalog = a_listeners_catalog(now);

        let offered = offer(&Searching::default(), &catalog, &Calendar::utc(), now).expect("lists");
        let shelves: Vec<usize> = offered.iter().map(shelf_position).collect();

        let mut in_order = shelves.clone();
        in_order.sort_unstable();
        assert_eq!(shelves, in_order);
        assert_eq!(shelves.first(), Some(&0));
    }

    #[test]
    fn what_is_offered_to_discover_turns_a_day_at_a_time_and_reads_the_same_all_day() {
        let now = SystemTime::now();
        let catalog = a_listeners_catalog(now);
        let discovered = |at: SystemTime| -> Vec<String> {
            let offered =
                offer(&Searching::default(), &catalog, &Calendar::utc(), at).expect("lists");

            names(&offered)
                .into_iter()
                .filter(|name| name.starts_with("Discover "))
                .map(str::to_owned)
                .collect()
        };

        let today = discovered(now);

        assert!(!today.is_empty());
        assert_eq!(today, discovered(now));
        assert_ne!(today, discovered(now + A_DAY));
    }

    #[test]
    fn a_catalog_that_has_not_moved_hands_the_same_lists_back() {
        let now = SystemTime::now();
        let catalog = a_listeners_catalog(now);
        let searching = Searching::default();

        let first = offer(&searching, &catalog, &Calendar::utc(), now).expect("lists");
        let again = offer(&searching, &catalog, &Calendar::utc(), now).expect("lists");
        let tomorrow = offer(&searching, &catalog, &Calendar::utc(), now + A_DAY).expect("lists");

        assert!(Arc::ptr_eq(&first, &again));
        assert!(!Arc::ptr_eq(&first, &tomorrow));
    }

    #[test]
    fn a_listen_counted_changes_what_is_offered() {
        let now = SystemTime::now();
        let catalog = a_listeners_catalog(now);
        let searching = Searching::default();
        let before = offer(&searching, &catalog, &Calendar::utc(), now).expect("lists");
        assert_eq!(songs_of(&before, "On repeat"), 12);

        shelve(&catalog, "Brand New", "Dub", 10, 3, A_DAY, now);
        let after = offer(&searching, &catalog, &Calendar::utc(), now).expect("lists");

        assert_eq!(songs_of(&after, "On repeat"), 22);
    }

    fn dated(catalog: &Catalog, artist: &str, date: &str) {
        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET date = ?2 WHERE artist = ?1",
                params![artist, date],
            )
            .expect("a date");
    }

    fn offered_in(offered: &[Offered], shelf: SuggestionShelf) -> Vec<&str> {
        offered
            .iter()
            .filter(|held| SuggestionShelf::from(held.candidate.reason.kind()) == shelf)
            .map(|held| held.candidate.name.as_str())
            .collect()
    }

    #[test]
    fn the_eras_genres_artists_and_sounds_of_the_catalog_are_offered_as_the_desktop_offers_them() {
        let now = SystemTime::now();
        let catalog = a_listeners_catalog(now);
        dated(&catalog, "Played Lately", "1975-03-01");
        dated(&catalog, "A Neighbour", "1976");
        dated(&catalog, "Long Gone", "1989-05-05");
        shelve(&catalog, "Cut One", "Punk", 6, 0, A_WEEK, now);
        shelve(&catalog, "Cut Two", "Grunge", 6, 0, A_WEEK, now);
        catalog
            .0
            .lock()
            .execute(
                "UPDATE documents SET duration_ms = 90000 WHERE artist LIKE 'Cut %'",
                [],
            )
            .expect("short songs");
        let searching = Searching::default();

        let offered = offer(&searching, &catalog, &Calendar::utc(), now).expect("lists");

        assert_eq!(
            offered_in(&offered, SuggestionShelf::Eras)[0],
            "The seventies"
        );
        assert_eq!(songs_of(&offered, "The seventies"), 25);
        assert!(offered_in(&offered, SuggestionShelf::Genres).contains(&"Krautrock"));
        assert_eq!(songs_of(&offered, "Krautrock"), 25);
        assert!(offered_in(&offered, SuggestionShelf::Artists).contains(&"Played Lately"));
        assert_eq!(songs_of(&offered, "Short and sharp"), 12);
        assert!(offered_in(&offered, SuggestionShelf::Sound).contains(&"Short and sharp"));
        for held in offered.iter() {
            let opened = searching
                .saved_page(&catalog, &held.candidate.query, 0, 500)
                .expect("the list opens");
            assert_eq!(
                opened.len() as u32,
                held.weighed.rows.min(500),
                "{}",
                held.candidate.name
            );
        }
    }

    #[test]
    fn a_catalog_too_small_to_fill_a_list_is_offered_none() {
        let now = SystemTime::now();
        let catalog = Catalog::open(Path::new(":memory:")).expect("a catalog");
        shelve(&catalog, "Few", "Dub", 3, 3, A_DAY, now);

        let offered = offer(&Searching::default(), &catalog, &Calendar::utc(), now).expect("lists");

        assert!(offered.is_empty());
    }

    #[test]
    fn a_suggestion_names_its_shelf_and_reason_as_the_desktop_does() {
        let now = SystemTime::now();
        let catalog = a_listeners_catalog(now);

        let offered = offer(&Searching::default(), &catalog, &Calendar::utc(), now).expect("lists");
        let on_repeat = offered
            .iter()
            .find(|held| held.candidate.name == "On repeat")
            .expect("on repeat")
            .listed();

        assert_eq!(on_repeat.reason, SuggestionReason::OnRepeat);
        assert_eq!(on_repeat.shelf, SuggestionShelf::ForYou);
        assert_eq!(on_repeat.shelf_name, "Made for you");
        assert_eq!(on_repeat.detail, Reason::OnRepeat.says());
        assert_eq!(on_repeat.search.text, "plays:>=3@1mo");
        assert_eq!(on_repeat.songs, 12);
    }
}
