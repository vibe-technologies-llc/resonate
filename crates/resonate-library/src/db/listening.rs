use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use resonate_core::{Calendar, FrameSpan, ListenId, MediaLocation, TrackId, wrapped::YearReview};
use rusqlite::{OptionalExtension as _, params};

use super::{KeptSuggestions, Library, Visited, rows::RawTrack, statements::TRACK_COLUMNS};
use crate::{
    Aged, Billed, Counted, Day, Error, Habits, HistoryKept, Listen, ListeningService, LovesTold,
    MostListened, Result, Scrobbler, Shared, Statistics, StoreOp, Submitted, Suggestion, Window,
    history, scrobble, share, statistics, store, suggest, wrapped,
};

impl Library {
    pub fn passed(&self, heard: Duration) -> Result<()> {
        let nanos = i64::try_from(heard.as_nanos()).unwrap_or(i64::MAX);

        self.inner.write(|transaction| {
            transaction
                .execute(
                    "INSERT INTO passes (at, heard) VALUES (?1, ?2)",
                    params![store::to_nanos(SystemTime::now()), nanos],
                )
                .map(drop)
                .map_err(|source| Error::store(StoreOp::Insert, source))
        })
    }

    pub fn listened(&self, listen: Listen, heard: Duration) -> Result<()> {
        let nanos = i64::try_from(heard.as_nanos()).unwrap_or(i64::MAX);

        self.inner
            .write(|transaction| history::spent(transaction, listen, nanos))
    }

    pub fn track_played(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
        heard: Duration,
    ) -> Result<Option<Counted>> {
        let Some(path) = location.as_path() else {
            return self.played_in_place(location, heard).map(Some);
        };
        let text = store::path_text(path);
        let (start, _) = store::span_columns(span);

        let raw = self.inner.write(|transaction| {
            let moment = SystemTime::now();
            let now = store::to_nanos(moment);
            let began = store::to_nanos(moment.checked_sub(heard).unwrap_or(moment));
            let counted = transaction
                .execute(
                    "UPDATE tracks SET plays = plays + 1, played = ?1
                     WHERE path = ?2 AND span_start = ?3",
                    params![now, text, start],
                )
                .map_err(|source| Error::store(StoreOp::Update, source))?;
            if counted == 0 {
                let unheld = transaction
                    .query_row(
                        "INSERT INTO unheld_listens (path, span_start, at, began)
                         VALUES (?1, ?2, ?3, ?4)
                         RETURNING id",
                        params![text, start, now, began],
                        |row| row.get::<_, i64>(0),
                    )
                    .map_err(|source| Error::store(StoreOp::Insert, source))?;
                return Ok(Visited::Unheld(unheld));
            }

            let listen = transaction
                .query_row(
                    "INSERT INTO listens (track_id, at, began)
                     SELECT id, ?1, ?4 FROM tracks WHERE path = ?2 AND span_start = ?3
                     RETURNING id",
                    params![now, text, start, began],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))?;

            transaction
                .query_row(
                    &format!(
                        "SELECT {TRACK_COLUMNS} FROM tracks WHERE path = ?1 AND span_start = ?2"
                    ),
                    params![text, start],
                    RawTrack::read,
                )
                .optional()
                .map(|raw| Visited::Held(raw.map(Box::new), listen))
                .map_err(|source| Error::store(StoreOp::Query, source))
        })?;

        Ok(Some(match raw {
            Visited::Unheld(unheld) => Counted {
                track: None,
                listen: Listen::Unheld(ListenId::new(unheld as u64)?),
            },
            Visited::Held(raw, listen) => Counted {
                track: raw.map(|raw| raw.into_track()).transpose()?,
                listen: Listen::Held(ListenId::new(listen as u64)?),
            },
        }))
    }

    fn played_in_place(&self, location: &MediaLocation, heard: Duration) -> Result<Counted> {
        let unheld = self.inner.write(|transaction| {
            let moment = SystemTime::now();
            let began = store::to_nanos(moment.checked_sub(heard).unwrap_or(moment));
            transaction
                .query_row(
                    "INSERT INTO unheld_listens (path, span_start, at, began)
                     VALUES (?1, 0, ?2, ?3)
                     RETURNING id",
                    params![location.to_uri(), store::to_nanos(moment), began],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(|source| Error::store(StoreOp::Insert, source))
        })?;
        Ok(Counted {
            track: None,
            listen: Listen::Unheld(ListenId::new(unheld as u64)?),
        })
    }

    pub fn submit_listens(&self, scrobbler: &dyn Scrobbler) -> Result<Submitted> {
        scrobble::submit(&self.inner, scrobbler)
    }

    pub fn tell_earlier_listens(&self, service: ListeningService) -> Result<()> {
        scrobble::tell_earlier(&self.inner, service)
    }

    pub fn stop_telling(&self, service: ListeningService) -> Result<bool> {
        scrobble::stop_telling(&self.inner, service)
    }

    pub fn earlier_listens_owed(&self, service: ListeningService) -> Result<bool> {
        scrobble::earlier_owed(&self.inner, service)
    }

    pub fn tell_loves(&self, scrobbler: &dyn Scrobbler) -> Result<LovesTold> {
        scrobble::tell_loves(&self.inner, scrobbler)
    }

    pub fn billed_as(
        &self,
        location: &MediaLocation,
        span: Option<FrameSpan>,
    ) -> Result<Option<Billed>> {
        let Some(path) = location.as_path() else {
            return Ok(None);
        };
        let text = store::path_text(path);
        scrobble::billed_as(&self.inner, &text, store::span_columns(span).0)
    }

    pub fn age_the_history(&self, kept: HistoryKept) -> Result<Aged> {
        let now = SystemTime::now();
        if kept.before(now).is_some() {
            wrapped::freeze_the_finished_years(&self.inner, &Calendar::local(), kept, now)?;
        }

        history::age(&self.inner, kept, now)
    }

    pub fn statistics(&self, window: Window) -> Result<Statistics> {
        statistics::statistics(&self.inner, window)
    }

    pub fn most_listened(&self, window: Window, most: usize) -> Result<MostListened> {
        statistics::most_listened(&self.inner, window, most)
    }

    pub fn listening_by_day(&self, window: Window) -> Result<Vec<Day>> {
        statistics::listening_by_day(&self.inner, window)
    }

    pub fn habits(&self, window: Window) -> Result<Habits> {
        statistics::habits(&self.inner, window)
    }

    pub fn days_and_habits(&self, window: Window) -> Result<(Vec<Day>, Habits)> {
        statistics::days_and_habits(&self.inner, window)
    }

    pub fn years_heard(&self, calendar: &Calendar) -> Result<Vec<i64>> {
        wrapped::years_heard(&self.inner, calendar)
    }

    pub fn year_review(
        &self,
        year: i64,
        calendar: &Calendar,
        history: HistoryKept,
    ) -> Result<YearReview> {
        wrapped::year_review(&self.inner, year, calendar, history, SystemTime::now())
    }

    pub fn shareable(&self, track: TrackId) -> Result<Option<Shared>> {
        share::shareable(&self.inner, track)
    }

    pub fn suggestions(&self) -> Result<Arc<[Suggestion]>> {
        let stamp = self.inner.rows_stamp();
        let heard = self.inner.heard();
        let filters = self.music_filters();
        let now = SystemTime::now();
        let today = Calendar::local().day_of(now);
        if let Some(kept) = self.inner.suggested.lock().as_ref().filter(|kept| {
            kept.stamp.still_holds_at(stamp)
                && kept.heard == heard
                && kept.filters == filters
                && kept.today == today
        }) {
            return Ok(Arc::clone(&kept.suggestions));
        }

        let filing = self.inner.filing()?;
        let suggestions: Arc<[Suggestion]> =
            suggest::suggestions(&self.inner, &filing, today, now)?.into();
        *self.inner.suggested.lock() = Some(KeptSuggestions {
            stamp,
            heard,
            filters,
            today,
            suggestions: Arc::clone(&suggestions),
        });
        Ok(suggestions)
    }
}
