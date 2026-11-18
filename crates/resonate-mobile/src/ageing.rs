use std::time::SystemTime;

use resonate_core::seconds_since_the_epoch;
use resonate_library::HistoryKept;
use rusqlite::Connection;

use crate::{
    Error, MobilePlayer, Result, StoreOp, catalog_extras::the_listeners_calendar,
    wrapped::freeze_the_finished_years,
};

const FORGET_THE_LISTENS: &str = "DELETE FROM listens
      WHERE counted = 1
        AND began < ?1
        AND id <= coalesce((SELECT min(through) FROM submissions), id)";

const FORGET_THE_PASSES: &str = "DELETE FROM listens WHERE counted = 0 AND began < ?1";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct AgedHistory {
    pub listens: u64,
    pub passes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum KeptHistory {
    Forever,
    Days { days: u16 },
}

impl From<HistoryKept> for KeptHistory {
    fn from(kept: HistoryKept) -> Self {
        match kept {
            HistoryKept::Forever => Self::Forever,
            HistoryKept::Days(days) => Self::Days { days: days.get() },
        }
    }
}

impl From<KeptHistory> for HistoryKept {
    fn from(kept: KeptHistory) -> Self {
        match kept {
            KeptHistory::Forever => Self::Forever,
            KeptHistory::Days { days } => Self::for_days(i64::from(days)).unwrap_or_default(),
        }
    }
}

#[uniffi::export]
pub fn history_spans_offered() -> Vec<KeptHistory> {
    HistoryKept::OFFERED.into_iter().map(Into::into).collect()
}

#[uniffi::export]
impl MobilePlayer {
    pub fn age_history(
        &self,
        kept: KeptHistory,
        time_zone: String,
        utc_offset_seconds: i32,
    ) -> Result<AgedHistory> {
        let kept = HistoryKept::from(kept);
        *self.services.history_kept.lock() = kept;

        let now = SystemTime::now();
        let Some(before) = kept.before(now) else {
            return Ok(AgedHistory::default());
        };
        let calendar = the_listeners_calendar(&time_zone, utc_offset_seconds);
        let mut connection = self.catalog.0.lock();
        freeze_the_finished_years(&connection, &calendar, kept, calendar.date_at(now))?;
        forgotten_before(&mut connection, seconds_since_the_epoch(before))
    }
}

fn forgotten_before(connection: &mut Connection, before: i64) -> Result<AgedHistory> {
    let store = |source| Error::store(StoreOp::Write, source);
    let transaction = connection.transaction().map_err(store)?;
    let listens = transaction
        .execute(FORGET_THE_LISTENS, [before])
        .map_err(store)?;
    let passes = transaction
        .execute(FORGET_THE_PASSES, [before])
        .map_err(store)?;
    transaction.commit().map_err(store)?;

    Ok(AgedHistory {
        listens: listens as u64,
        passes: passes as u64,
    })
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use resonate_core::Calendar;

    use super::*;
    use crate::{
        history::tests::{heard_here, listened, listens, passed_over, songs_held},
        playlists::tests::player,
    };

    const A_DAY: i64 = 86_400;

    fn the_listens_began(player: &MobilePlayer) -> Vec<(i64, bool)> {
        listens(player)
            .into_iter()
            .map(|(_, began, _, counted, _)| (began, counted))
            .collect()
    }

    #[test]
    fn ageing_forgets_listens_and_passes_past_the_span_but_not_a_listen_a_service_is_owed() {
        let phone = player();
        let [echoes, _] = songs_held(&phone);
        let now = seconds_since_the_epoch(SystemTime::now());
        let old = now - 400 * A_DAY;
        let recent = now - 10 * A_DAY;
        heard_here(&phone, echoes, old, 200_000);
        passed_over(&phone, old + 60);
        heard_here(&phone, echoes, recent, 200_000);
        passed_over(&phone, recent + 60);
        heard_here(&phone, echoes, old + 120, 200_000);
        let owed_from: i64 = phone
            .catalog
            .0
            .lock()
            .query_row("SELECT max(id) FROM listens", [], |row| row.get(0))
            .expect("the newest");
        listened(
            &phone,
            "INSERT INTO submissions (service, through) VALUES ('lastfm', ?1)",
            [owed_from - 1],
        );

        let forever = phone
            .age_history(KeptHistory::Forever, "UTC".into(), 0)
            .expect("kept whole");
        let a_year = phone
            .age_history(KeptHistory::Days { days: 365 }, "UTC".into(), 0)
            .expect("aged");

        assert_eq!(forever, AgedHistory::default());
        assert_eq!(
            a_year,
            AgedHistory {
                listens: 1,
                passes: 1,
            }
        );
        assert_eq!(
            the_listens_began(&phone),
            [(old + 120, true), (recent, true), (recent + 60, false)]
        );
        assert_eq!(
            *phone.services.history_kept.lock(),
            HistoryKept::for_days(365).expect("a span")
        );
    }

    #[test]
    fn ageing_keeps_a_year_over_as_it_was_heard_before_forgetting_its_listens() {
        let phone = player();
        let [echoes, _] = songs_held(&phone);
        let three_years_ago = seconds_since_the_epoch(SystemTime::now()) - 3 * 366 * A_DAY;
        heard_here(&phone, echoes, three_years_ago, 3_600_000);
        let year = Calendar::utc()
            .date_at(UNIX_EPOCH + Duration::from_secs(three_years_ago.cast_unsigned()))
            .year;

        phone
            .age_history(KeptHistory::Days { days: 182 }, "UTC".into(), 0)
            .expect("aged");
        let shown = phone
            .wrapped(Some(year), "UTC".into(), 0)
            .expect("the catalog answers")
            .expect("the year kept");

        assert!(listens(&phone).is_empty());
        assert_eq!(shown.slides[0].headline, "1 hour of music");
    }

    #[test]
    fn the_spans_offered_are_the_desktops_and_each_reads_back_as_itself() {
        let offered = history_spans_offered();

        assert_eq!(offered.first(), Some(&KeptHistory::Forever));
        assert_eq!(offered.len(), HistoryKept::OFFERED.len());
        for kept in offered {
            assert_eq!(KeptHistory::from(HistoryKept::from(kept)), kept);
        }
        assert_eq!(
            HistoryKept::from(KeptHistory::Days { days: 0 }),
            HistoryKept::Forever
        );
    }
}
