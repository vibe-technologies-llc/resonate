use std::time::Duration;

use resonate_library::{Library, Listened, MostListened, Statistics, Window};

use crate::{Result, cli::WindowArg, table::Table, words};

const SECONDS_A_MINUTE: u64 = 60;
const MINUTES_AN_HOUR: u64 = 60;
const SECONDS_AN_HOUR: u64 = MINUTES_AN_HOUR * SECONDS_A_MINUTE;

pub fn print(library: &Library, window: WindowArg, most: usize) -> Result<()> {
    let window = Window::from(window);
    let counts = library.statistics(window)?;
    said!("{}", summarised(counts, window));

    if counts.plays == 0 {
        return Ok(());
    }
    let listened = library.most_listened(window, most)?;
    let MostListened {
        tracks,
        albums,
        artists,
    } = &listened;

    section(words::stats::tracks_most(), words::stats::track(), tracks);
    section(words::stats::albums_most(), words::stats::album(), albums);
    section(
        words::stats::artists_most(),
        words::stats::artist(),
        artists,
    );
    Ok(())
}

fn section<Id>(heading: &str, named: &'static str, rows: &[Listened<Id>]) {
    said!();
    said!("{heading}");
    if rows.is_empty() {
        said!("{}", words::stats::nothing_played());
        return;
    }

    let mut table = Table::new(vec![
        named,
        words::stats::plays_heading(),
        words::stats::listened_heading(),
    ]);
    for row in rows {
        table.push(vec![
            row.name.clone(),
            row.plays.to_string(),
            heard_for(row.listened),
        ]);
    }
    said_on!("{}", table.render());
}

fn summarised(counts: Statistics, window: Window) -> String {
    let across = across(window);
    if counts.plays == 0 {
        return words::stats::nothing_played_across(across);
    }

    words::stats::summary(
        across,
        counts.albums,
        counts.artists,
        heard_for(counts.listened),
        counts.plays,
        counts.tracks,
    )
}

fn across(window: Window) -> &'static str {
    match window {
        Window::Everything => words::stats::all_told(),
        Window::Week => words::stats::over_the_last_week(),
        Window::Month => words::stats::over_the_last_month(),
        Window::Year => words::stats::over_the_last_year(),
    }
}

fn heard_for(span: Duration) -> String {
    let seconds = span.as_secs();
    let minutes = seconds / SECONDS_A_MINUTE;

    match seconds / SECONDS_AN_HOUR {
        0 if minutes == 0 => words::stats::seconds(seconds),
        0 => words::stats::minutes(minutes),
        hours => words::stats::hours_and_minutes(hours, minutes % MINUTES_AN_HOUR),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_listening_time_is_drawn_in_hours_rather_than_minutes() {
        let many_hours = Duration::from_secs(413 * SECONDS_AN_HOUR + 27 * SECONDS_A_MINUTE + 9);

        assert_eq!(heard_for(many_hours), "413h 27m");
        assert_eq!(heard_for(Duration::from_secs(SECONDS_AN_HOUR)), "1h 0m");
    }

    #[test]
    fn a_listening_time_short_of_an_hour_is_drawn_in_minutes_and_one_short_of_a_minute_in_seconds()
    {
        assert_eq!(heard_for(Duration::from_secs(3_599)), "59m");
        assert_eq!(heard_for(Duration::from_secs(60)), "1m");
        assert_eq!(heard_for(Duration::from_secs(59)), "59s");
        assert_eq!(heard_for(Duration::ZERO), "0s");
    }

    #[test]
    fn a_window_with_nothing_in_it_says_so_rather_than_drawing_empty_tables() {
        assert_eq!(
            summarised(Statistics::default(), Window::Week),
            "nothing was played over the last week"
        );
        assert_eq!(
            summarised(Statistics::default(), Window::Everything),
            "nothing was played all told"
        );
    }

    #[test]
    fn a_summary_counts_each_thing_in_its_own_number() {
        let counts = Statistics {
            plays: 1,
            listened: Duration::from_secs(212),
            tracks: 1,
            albums: 2,
            artists: 3,
        };

        assert_eq!(
            summarised(counts, Window::Everything),
            "1 play all told · 3m listened · 1 track · 2 albums · 3 artists"
        );
    }
}
