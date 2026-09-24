use rusqlite::{OptionalExtension, Transaction, params};

use crate::{
    error::{Error, Result, StoreOp},
    store::folded_letters,
};

const JOINS: &[&str] = &[
    " & ",
    " and ",
    ", ",
    "; ",
    " / ",
    " + ",
    " x ",
    " × ",
    " with ",
    " feat. ",
    " feat ",
    " ft. ",
    " ft ",
    " featuring ",
    " vs. ",
    " vs ",
];

pub fn members_of(credit: &str) -> Vec<&str> {
    let mut members = Vec::new();
    let mut rest = credit;
    while let Some((at, join)) = next_join(rest) {
        members.push(rest[..at].trim());
        rest = &rest[at + join.len()..];
    }
    members.push(rest.trim());
    members.retain(|member| !member.is_empty());
    members
}

fn next_join(text: &str) -> Option<(usize, &'static str)> {
    let lowered = text.to_ascii_lowercase();
    JOINS
        .iter()
        .filter_map(|join| lowered.find(join).map(|at| (at, *join)))
        .min_by_key(|(at, join)| (*at, usize::MAX - join.len()))
}

pub(crate) fn credit_the_members(tx: &Transaction<'_>) -> Result<()> {
    tx.execute("DELETE FROM track_credits", [])
        .map_err(|source| Error::store(StoreOp::Delete, source))?;

    for credit in credits_held(tx)? {
        let members = members_of(&credit);
        if members.len() < 2 {
            continue;
        }
        let Some(artists) = every_member_held(tx, &credit, &members)? else {
            continue;
        };
        credit_each(tx, &credit, &artists)?;
    }
    Ok(())
}

fn credits_held(tx: &Transaction<'_>) -> Result<Vec<String>> {
    tx.prepare("SELECT DISTINCT artist FROM tracks WHERE artist IS NOT NULL")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get(0))
                .and_then(Iterator::collect::<rusqlite::Result<Vec<String>>>)
        })
        .map_err(|source| Error::store(StoreOp::Query, source))
}

fn every_member_held(
    tx: &Transaction<'_>,
    credit: &str,
    members: &[&str],
) -> Result<Option<Vec<i64>>> {
    let whole = folded_letters(credit);
    let mut artists: Vec<i64> = Vec::with_capacity(members.len());
    for member in members {
        let key = folded_letters(member);
        if key.is_empty() || key == whole {
            return Ok(None);
        }
        let Some(artist) = artist_keyed(tx, &key)? else {
            return Ok(None);
        };
        if !artists.contains(&artist) {
            artists.push(artist);
        }
    }
    Ok((artists.len() > 1).then_some(artists))
}

fn artist_keyed(tx: &Transaction<'_>, key: &str) -> Result<Option<i64>> {
    tx.query_row(
        "SELECT id FROM artists WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
    .map_err(|source| Error::store(StoreOp::Query, source))
}

fn credit_each(tx: &Transaction<'_>, credit: &str, artists: &[i64]) -> Result<()> {
    for artist in artists {
        tx.execute(
            "INSERT OR IGNORE INTO track_credits (track_id, artist_id)
             SELECT id, ?2 FROM tracks WHERE artist = ?1",
            params![credit, artist],
        )
        .map_err(|source| Error::store(StoreOp::Insert, source))?;
    }
    let lead = artists[0];
    tx.execute(
        "UPDATE tracks SET artist_id = ?2
          WHERE artist = ?1
            AND (artist_id IS NULL
                 OR artist_id NOT IN (SELECT artist_id FROM track_credits c
                                       WHERE c.track_id = tracks.id))",
        params![credit, lead],
    )
    .map(drop)
    .map_err(|source| Error::store(StoreOp::Update, source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credit_is_read_as_the_names_its_joins_separate() {
        assert_eq!(
            members_of("Adam Skorupa & Krzysztof Wierzynkiewicz"),
            vec!["Adam Skorupa", "Krzysztof Wierzynkiewicz"]
        );
        assert_eq!(
            members_of("Joris de Man; Julie Elven"),
            vec!["Joris de Man", "Julie Elven"]
        );
        assert_eq!(
            members_of("Percival, Marcin Przybyłowicz and Mikolai Stroinski"),
            vec!["Percival", "Marcin Przybyłowicz", "Mikolai Stroinski"]
        );
        assert_eq!(
            members_of("Kanye West Feat. Jay-Z"),
            vec!["Kanye West", "Jay-Z"]
        );
    }

    #[test]
    fn a_name_with_no_join_in_it_is_one_member() {
        assert_eq!(
            members_of("Marcin Przybyłowicz"),
            vec!["Marcin Przybyłowicz"]
        );
        assert_eq!(members_of("Andrew"), vec!["Andrew"]);
        assert_eq!(members_of("Xavier"), vec!["Xavier"]);
        assert!(members_of("  ").is_empty());
    }
}
