use resonate_core::{
    Calendar,
    wrapped::{Slide, YearReview, year_worth_showing},
};
use resonate_library::{HistoryKept, Library};

use crate::{Error, Result, table::Table, words::columns};

pub fn print(
    library: &Library,
    year: Option<i64>,
    summary: bool,
    history: HistoryKept,
) -> Result<()> {
    let calendar = Calendar::local();
    let years = library.years_heard(&calendar)?;
    let today = calendar.date_at(std::time::SystemTime::now());
    let Some(year) = year.or_else(|| year_worth_showing(&years, today)) else {
        return Err(Error::NothingHeardYet);
    };

    let review = library.year_review(year, &calendar, history)?;
    if summary {
        said!("{}", review.summary());
    } else {
        said_on!("{}", told(&review));
    }

    Ok(())
}

fn told(review: &YearReview) -> String {
    review
        .slides()
        .iter()
        .map(told_slide)
        .collect::<Vec<_>>()
        .join("\n")
}

fn told_slide(slide: &Slide) -> String {
    let mut told = format!("{}\n{}\n", slide.eyebrow.to_uppercase(), slide.headline);
    if let Some(detail) = &slide.detail {
        told.push_str(detail);
        told.push('\n');
    }
    if !slide.rows.is_empty() {
        let mut table = Table::new(vec!["", columns::by(), columns::listened()]);
        for row in &slide.rows {
            table.push(vec![
                row.name.clone(),
                row.by.clone().unwrap_or_default(),
                row.figure.clone(),
            ]);
        }
        told.push_str(&table.render());
    }
    for bar in slide.bars.iter().filter(|bar| bar.share_permille > 0) {
        told.push_str(&bar.said);
        told.push('\n');
    }

    told
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use resonate_core::wrapped::Ranked;

    use super::*;

    #[test]
    fn a_year_is_told_slide_by_slide_with_its_rankings_as_tables() {
        let review = YearReview {
            year: 2026,
            listened: Duration::from_secs(3 * 3_600),
            plays: 30,
            songs: 3,
            artists: 1,
            albums: 1,
            top_songs: vec![Ranked {
                name: "Echoes".to_owned(),
                by: Some("Pink Floyd".to_owned()),
                plays: 30,
                listened: Duration::from_secs(3 * 3_600),
            }],
            ..YearReview::default()
        };

        let told = told(&review);

        assert!(told.contains("RESONATE WRAPPED 2026\n3 hours of music\n"));
        assert!(told.contains("YOUR SONG OF THE YEAR\nEchoes\nPink Floyd\n"));
        assert!(told.contains("That was your 2026"));
    }

    #[test]
    fn a_year_with_nothing_in_it_says_so_rather_than_drawing_empty_tables() {
        let told = told(&YearReview {
            year: 2026,
            ..YearReview::default()
        });

        assert!(told.contains("Nothing was played in 2026"));
        assert!(!told.contains("LISTENED"));
    }
}
