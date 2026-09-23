use std::time::Duration;

use resonate_library::{Library, Listened, MostListened, Statistics, Window};

use crate::{Result, cli::WindowArg, counted, table::Table};

const SECONDS_A_MINUTE: u64 = 60;
const MINUTES_AN_HOUR: u64 = 60;
const SECONDS_AN_HOUR: u64 = MINUTES_AN_HOUR * SECONDS_A_MINUTE;

const NOTHING_WAS_HEARD: &str = "nothing was played";

pub fn print(library: &Library, window: WindowArg, most: usize) -> Result<()> {
    let window = Window::from(window);
    let counts = library.statistics(window)?;
    println!("{}", summarised(counts, window));

    if counts.plays == 0 {
        return Ok(());
    }
    let listened = library.most_listened(window, most)?;
    let MostListened {
        tracks,
        albums,
        artists,
    } = &listened;

    section("TRACKS MOST LISTENED TO", "TRACK", tracks);
    section("ALBUMS MOST LISTENED TO", "ALBUM", albums);
    section("ARTISTS MOST LISTENED TO", "ARTIST", artists);
    Ok(())
}

fn section<Id>(heading: &str, named: &'static str, rows: &[Listened<Id>]) {
    println!();
    println!("{heading}");
    if rows.is_empty() {
        println!("{NOTHING_WAS_HEARD}");
        return;
    }

    let mut table = Table::new(vec![named, "PLAYS", "LISTENED"]);
    for row in rows {
        table.push(vec![
            row.name.clone(),
            row.plays.to_string(),
            heard_for(row.listened),
        ]);
    }
    print!("{}", table.render());
}

fn summarised(counts: Statistics, window: Window) -> String {
    let across = across(window);
    if counts.plays == 0 {
        return format!("{NOTHING_WAS_HEARD} {across}");
    }

    format!(
        "{} {across} · {} listened · {} · {} · {}",
        counted(counts.plays, "play", "plays"),
        heard_for(counts.listened),
        counted(counts.tracks, "track", "tracks"),
        counted(counts.albums, "album", "albums"),
        counted(counts.artists, "artist", "artists")
    )
}

fn across(window: Window) -> String {
    match window {
        Window::Everything => "all told".to_owned(),
        span => format!("over the last {}", span.name()),
    }
}

fn heard_for(span: Duration) -> String {
    let seconds = span.as_secs();
    let minutes = seconds / SECONDS_A_MINUTE;

    match seconds / SECONDS_AN_HOUR {
        0 if minutes == 0 => format!("{seconds}s"),
        0 => format!("{minutes}m"),
        hours => format!("{hours}h {}m", minutes % MINUTES_AN_HOUR),
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
