use std::ops::Range;

use ahash::AHashMap;

use crate::{
    Clause, Column, Condition, Search, Word,
    search::{lettered_runs, runs_in},
    store,
};

const NEVER_CORRECTED_ABOVE: usize = 3;

const ONE_LETTER_WRONG_UNTIL: usize = 7;

const FURTHEST: usize = 2;

const MOST_PIECES: usize = 4;

const MOST_TOKENS_IN_A_NAME: usize = 8;

const BETWEEN_RUNS: char = ' ';

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Spellings {
    titles: Vocabulary,
    artists: Vocabulary,
    albums: Vocabulary,
    genres: Vocabulary,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Vocabulary {
    spelled: AHashMap<String, Spelled>,
    named: AHashMap<String, Spelled>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Spelled {
    spelling: String,
    rows: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Nearness {
    apart: usize,
    rows: std::cmp::Reverse<u32>,
}

struct Instead {
    written: String,
    runs: usize,
    widened: bool,
}

#[derive(Clone, Copy)]
struct Reached {
    rows: u32,
    from: usize,
}

impl Vocabulary {
    pub fn taking(&mut self, name: &str) {
        let mut whole = String::with_capacity(name.len());
        for (spelling, folded) in runs_in(name) {
            if !whole.is_empty() {
                whole.push(BETWEEN_RUNS);
            }
            whole.push_str(&folded);
            take(&mut self.spelled, folded, spelling);
        }

        if whole.contains(BETWEEN_RUNS) {
            take(&mut self.named, whole, name.trim());
        }
    }

    fn holds(&self, run: &str) -> bool {
        begins_one_of(&self.spelled, run)
    }

    fn names(&self, whole: &str) -> bool {
        begins_one_of(&self.named, whole)
    }

    fn spelt(&self, run: &str) -> Option<&Spelled> {
        self.spelled.get(run)
    }

    fn nearest(&self, run: &str) -> Option<(&str, Nearness)> {
        nearest_in(&self.spelled, run)
    }

    fn nearest_name(&self, whole: &str) -> Option<(&str, Nearness)> {
        nearest_in(&self.named, whole)
    }
}

fn take(into: &mut AHashMap<String, Spelled>, folded: String, spelling: &str) {
    let held = into.entry(folded).or_insert_with(|| Spelled {
        spelling: spelling.to_owned(),
        rows: 0,
    });
    held.rows = held.rows.saturating_add(1);
    if better_spelt(spelling, &held.spelling) {
        held.spelling = spelling.to_owned();
    }
}

fn begins_one_of(held: &AHashMap<String, Spelled>, run: &str) -> bool {
    held.contains_key(run) || held.keys().any(|spelt| spelt.starts_with(run))
}

fn nearest_in<'a>(held: &'a AHashMap<String, Spelled>, run: &str) -> Option<(&'a str, Nearness)> {
    let furthest = furthest_from(letters_in(run))?;
    let mut nearest: Option<(&str, Nearness)> = None;

    for (folded, spelt) in held {
        let Some(apart) = apart_by(run, folded, furthest) else {
            continue;
        };
        let weighed = Nearness {
            apart,
            rows: std::cmp::Reverse(spelt.rows),
        };
        let takes = nearest.is_none_or(|(spelling, standing)| {
            (weighed, spelt.spelling.as_str()) < (standing, spelling)
        });
        if takes {
            nearest = Some((spelt.spelling.as_str(), weighed));
        }
    }

    nearest
}

pub(crate) fn worth_asking(search: &Search) -> bool {
    let folded: Vec<String> = search
        .clauses
        .iter()
        .flat_map(|clause| clause.any.iter())
        .filter(|asked| !asked.denied)
        .flat_map(|asked| asked.all.iter())
        .filter_map(|condition| match condition {
            Condition::Word(word) => Some(word),
            Condition::Term(_) => None,
        })
        .flat_map(|word| lettered_runs(&word.text))
        .map(|(_, folded)| folded)
        .collect();

    folded
        .iter()
        .any(|run| furthest_from(letters_in(run)).is_some())
        || folded
            .windows(2)
            .any(|pair| furthest_from(letters_in(&pair[0]) + letters_in(&pair[1])).is_some())
}

impl Spellings {
    pub fn taking(&mut self, column: Column, name: &str) {
        if let Some(vocabulary) = self.of(column) {
            vocabulary.taking(name);
        }
    }

    const fn of(&mut self, column: Column) -> Option<&mut Vocabulary> {
        match column {
            Column::Title => Some(&mut self.titles),
            Column::Artist => Some(&mut self.artists),
            Column::Album => Some(&mut self.albums),
            Column::Genre => Some(&mut self.genres),
            Column::Lyrics => None,
        }
    }

    fn reading(&self, column: Option<Column>) -> Vec<&Vocabulary> {
        match column {
            Some(Column::Title) => vec![&self.titles],
            Some(Column::Artist) => vec![&self.artists],
            Some(Column::Album) => vec![&self.albums],
            Some(Column::Genre) => vec![&self.genres],
            Some(Column::Lyrics) => Vec::new(),
            None => vec![&self.titles, &self.artists, &self.albums, &self.genres],
        }
    }

    fn instead_of(&self, run: &str, column: Option<Column>) -> Option<String> {
        let reading = self.reading(column);
        if reading.iter().any(|vocabulary| vocabulary.holds(run)) {
            return None;
        }

        reading
            .iter()
            .filter_map(|vocabulary| vocabulary.nearest(run))
            .min_by(|(spelling, weighed), (other, standing)| {
                (weighed, spelling).cmp(&(standing, other))
            })
            .map(|(spelling, _)| spelling.to_owned())
    }

    fn whole(&self, run: &str, column: Option<Column>) -> Option<&Spelled> {
        self.reading(column)
            .into_iter()
            .filter_map(|vocabulary| vocabulary.spelt(run))
            .max_by(|held, other| {
                (held.rows, store::marks_in(&held.spelling), &held.spelling).cmp(&(
                    other.rows,
                    store::marks_in(&other.spelling),
                    &other.spelling,
                ))
            })
    }

    fn run_together(&self, one: &str, other: &str, column: Option<Column>) -> Option<String> {
        if self.whole(one, column).is_some() && self.whole(other, column).is_some() {
            return None;
        }

        let mut joined = String::with_capacity(one.len() + other.len());
        joined.push_str(one);
        joined.push_str(other);

        self.whole(&joined, column)
            .map(|held| held.spelling.clone())
    }

    fn instead_of_the_whole(&self, text: &str, column: Option<Column>) -> Option<String> {
        let runs = lettered_runs(text);
        if runs.len() < 2 {
            return None;
        }

        let whole = runs
            .iter()
            .map(|(_, folded)| folded.as_str())
            .collect::<Vec<_>>()
            .join(&BETWEEN_RUNS.to_string());

        self.instead_of_the_name(&whole, column)
    }

    fn instead_of_the_name(&self, whole: &str, column: Option<Column>) -> Option<String> {
        let reading = self.reading(column);
        if reading.iter().any(|vocabulary| vocabulary.names(whole)) {
            return None;
        }

        reading
            .iter()
            .filter_map(|vocabulary| vocabulary.nearest_name(whole))
            .min_by(|(spelling, weighed), (other, standing)| {
                (weighed, spelling).cmp(&(standing, other))
            })
            .map(|(spelling, _)| spelling.to_owned())
    }

    fn split_apart(&self, run: &str, column: Option<Column>) -> Option<String> {
        let reading = self.reading(column);
        if reading.iter().any(|vocabulary| vocabulary.holds(run)) {
            return None;
        }

        furthest_from(letters_in(run))?;
        let letters: Vec<char> = run.chars().collect();
        let mut reached = vec![vec![None; MOST_PIECES + 1]; letters.len() + 1];
        reached[0][0] = Some(Reached { rows: 0, from: 0 });

        for to in 1..=letters.len() {
            for from in 0..to {
                let piece: String = letters[from..to].iter().collect();
                let Some(held) = self.whole(&piece, column) else {
                    continue;
                };
                for pieces in 1..=MOST_PIECES {
                    let Some(before) = reached[from][pieces - 1] else {
                        continue;
                    };
                    let rows = before.rows.saturating_add(held.rows);
                    if reached[to][pieces].is_none_or(|standing: Reached| rows > standing.rows) {
                        reached[to][pieces] = Some(Reached { rows, from });
                    }
                }
            }
        }

        let pieces = (2..=MOST_PIECES).find(|pieces| reached[letters.len()][*pieces].is_some())?;
        Some(self.written_as(&letters, &reached, pieces, column))
    }

    fn written_as(
        &self,
        letters: &[char],
        reached: &[Vec<Option<Reached>>],
        pieces: usize,
        column: Option<Column>,
    ) -> String {
        let mut spelt = Vec::with_capacity(pieces);
        let mut to = letters.len();

        for left in (1..=pieces).rev() {
            let step = reached[to][left].expect("a cut the walk reached");
            let piece: String = letters[step.from..to].iter().collect();
            spelt.push(
                self.whole(&piece, column)
                    .map_or(piece, |held| held.spelling.clone()),
            );
            to = step.from;
        }

        spelt.reverse();
        spelt.join(&BETWEEN_RUNS.to_string())
    }

    fn instead_of_runs(
        &self,
        runs: &[(Range<usize>, String)],
        at: usize,
        column: Option<Column>,
    ) -> Option<Instead> {
        if let Some(next) = runs.get(at + 1)
            && let Some(written) = self.run_together(&runs[at].1, &next.1, column)
        {
            return Some(Instead {
                written,
                runs: 2,
                widened: false,
            });
        }
        if let Some(written) = self.instead_of(&runs[at].1, column) {
            return Some(Instead {
                written,
                runs: 1,
                widened: false,
            });
        }

        let written = self.split_apart(&runs[at].1, column)?;
        Some(Instead {
            written,
            runs: 1,
            widened: true,
        })
    }

    pub fn did_you_mean(&self, text: &str) -> Option<String> {
        let mut search = Search::read(text);
        let mut corrected = self.run_tokens_together(&mut search);
        corrected |= self.name_the_tokens(&mut search);

        for clause in &mut search.clauses {
            for asked in &mut clause.any {
                if asked.denied {
                    continue;
                }
                for condition in &mut asked.all {
                    let Condition::Word(word) = condition else {
                        continue;
                    };
                    corrected |= self.correct(word);
                }
            }
        }

        corrected.then(|| search.to_string())
    }

    fn run_tokens_together(&self, search: &mut Search) -> bool {
        let mut joined = false;
        let mut at = 0;

        while at + 1 < search.clauses.len() {
            let taken = one_run_of(&search.clauses[at])
                .zip(one_run_of(&search.clauses[at + 1]))
                .filter(|((column, _), (beside, _))| column == beside)
                .and_then(|((column, one), (_, other))| self.run_together(&one, &other, *column));
            let Some(written) = taken else {
                at += 1;
                continue;
            };

            let Some(Condition::Word(word)) = search.clauses[at].any[0].all.first_mut() else {
                at += 1;
                continue;
            };
            word.text = written;
            search.clauses.remove(at + 1);
            joined = true;
        }

        joined
    }

    fn name_the_tokens(&self, search: &mut Search) -> bool {
        let mut named = false;
        let mut at = 0;

        while at < search.clauses.len() {
            let Some((span, written)) = self.names_the_run(search, at) else {
                at += 1;
                continue;
            };
            let Some(Condition::Word(word)) = search.clauses[at].any[0].all.first_mut() else {
                at += 1;
                continue;
            };
            word.phrase |= word.column.is_some() && written.contains(char::is_whitespace);
            word.text = written;
            search.clauses.drain(at + 1..at + span);
            named = true;
            at += 1;
        }

        named
    }

    fn names_the_run(&self, search: &Search, at: usize) -> Option<(usize, String)> {
        let mut runs: Vec<(String, bool)> = Vec::new();
        let mut column = None;

        for clause in search.clauses.iter().skip(at).take(MOST_TOKENS_IN_A_NAME) {
            let Some((held, folded)) = one_run_of(clause) else {
                break;
            };
            if runs.is_empty() {
                column = *held;
            } else if column != *held {
                break;
            }
            let beyond = self.beyond_a_word(&folded, column);
            runs.push((folded, beyond));
        }

        for span in (2..=runs.len()).rev() {
            let taken = &runs[..span];
            if !taken.iter().any(|(_, beyond)| *beyond) {
                continue;
            }

            let whole = taken
                .iter()
                .map(|(folded, _)| folded.as_str())
                .collect::<Vec<_>>()
                .join(&BETWEEN_RUNS.to_string());
            if let Some(written) = self.instead_of_the_name(&whole, column) {
                return Some((span, written));
            }
        }

        None
    }

    fn beyond_a_word(&self, run: &str, column: Option<Column>) -> bool {
        !self
            .reading(column)
            .iter()
            .any(|vocabulary| vocabulary.holds(run))
            && self.instead_of(run, column).is_none()
    }

    fn correct(&self, word: &mut Word) -> bool {
        if word.phrase
            && let Some(written) = self.instead_of_the_whole(&word.text, word.column)
        {
            word.text = written;
            return true;
        }

        let runs = lettered_runs(&word.text);
        let mut written = String::with_capacity(word.text.len());
        let mut corrected = false;
        let mut widened = false;
        let mut after = 0;
        let mut at = 0;

        while at < runs.len() {
            let Some(instead) = self.instead_of_runs(&runs, at, word.column) else {
                at += 1;
                continue;
            };
            let taken = at + instead.runs - 1;
            written.push_str(word.text.get(after..runs[at].0.start).unwrap_or_default());
            written.push_str(&instead.written);
            after = runs[taken].0.end;
            corrected = true;
            widened |= instead.widened;
            at = taken + 1;
        }
        if !corrected {
            return false;
        }

        written.push_str(word.text.get(after..).unwrap_or_default());
        word.text = written;
        word.phrase |= widened && word.column.is_some();
        true
    }
}

fn one_run_of(clause: &Clause) -> Option<(&Option<Column>, String)> {
    let [asked] = clause.any.as_slice() else {
        return None;
    };
    if asked.denied {
        return None;
    }
    let [Condition::Word(word)] = asked.all.as_slice() else {
        return None;
    };
    if word.phrase {
        return None;
    }
    let runs = lettered_runs(&word.text);
    let [(_, folded)] = runs.as_slice() else {
        return None;
    };

    Some((&word.column, folded.clone()))
}

fn better_spelt(spelling: &str, than: &str) -> bool {
    (store::marks_in(spelling), spelling) > (store::marks_in(than), than)
}

fn letters_in(run: &str) -> usize {
    run.chars().count()
}

const fn furthest_from(letters: usize) -> Option<usize> {
    match letters {
        0..=NEVER_CORRECTED_ABOVE => None,
        4..=ONE_LETTER_WRONG_UNTIL => Some(1),
        _ => Some(FURTHEST),
    }
}

fn apart_by(one: &str, other: &str, furthest: usize) -> Option<usize> {
    let left: Vec<char> = one.chars().collect();
    let right: Vec<char> = other.chars().collect();
    if left.len().abs_diff(right.len()) > furthest {
        return None;
    }

    let mut before = (0..=right.len()).collect::<Vec<usize>>();
    let mut last = vec![0; right.len() + 1];
    let mut row = vec![0; right.len() + 1];

    for (down, letter) in left.iter().enumerate() {
        row[0] = down + 1;
        let mut nearest = row[0];
        for (across, against) in right.iter().enumerate() {
            let swapped = usize::from(letter != against);
            let mut steps = (before[across] + swapped)
                .min(before[across + 1] + 1)
                .min(row[across] + 1);
            if down > 0 && across > 0 && letter == &right[across - 1] && &left[down - 1] == against
            {
                steps = steps.min(last[across - 1] + 1);
            }
            row[across + 1] = steps;
            nearest = nearest.min(steps);
        }
        if nearest > furthest {
            return None;
        }
        std::mem::swap(&mut last, &mut before);
        std::mem::swap(&mut before, &mut row);
    }

    Some(before[right.len()]).filter(|apart| *apart <= furthest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalogued() -> Spellings {
        let mut spellings = Spellings::default();
        spellings.taking(Column::Artist, "Pink Floyd");
        spellings.taking(Column::Artist, "Marcin Przybyłowicz");
        spellings.taking(Column::Album, "The Dark Side of the Moon");
        spellings.taking(Column::Title, "Echoes");
        spellings.taking(Column::Title, "The Great Gig in the Sky");
        spellings
    }

    #[test]
    fn a_scoped_name_read_across_tokens_is_offered_as_a_phrase_the_scope_reaches() {
        let offered = catalogued()
            .did_you_mean("artist:pnkk artist:floyd")
            .expect("a name the catalog holds");

        assert_eq!(offered, "artist:\"Pink Floyd\"");
        let read = Search::read(&offered);
        assert_eq!(
            read.clauses.len(),
            1,
            "the scope reached only the first word"
        );
    }

    #[test]
    fn a_word_in_another_script_is_weighed_by_its_letters_rather_than_its_bytes() {
        let mut spellings = Spellings::default();
        spellings.taking(Column::Title, "Мор");
        spellings.taking(Column::Title, "東京");
        spellings.taking(Column::Title, "Echoes");

        assert_eq!(spellings.did_you_mean("мир"), None);
        assert_eq!(spellings.did_you_mean("東北"), None);
        assert_eq!(spellings.did_you_mean("ekhoes"), Some("Echoes".to_owned()));
    }

    #[test]
    fn a_word_the_catalog_holds_is_left_alone() {
        assert_eq!(catalogued().did_you_mean("floyd"), None);
        assert_eq!(catalogued().did_you_mean("echoes"), None);
    }

    #[test]
    fn a_word_that_is_the_beginning_of_one_is_left_alone() {
        assert_eq!(catalogued().did_you_mean("floy"), None);
        assert_eq!(catalogued().did_you_mean("przyby"), None);
    }

    #[test]
    fn a_misspelt_word_is_answered_in_the_spelling_the_catalog_holds() {
        assert_eq!(
            catalogued().did_you_mean("floid"),
            Some("Floyd".to_owned()),
            "a letter wrong was not corrected"
        );
        assert_eq!(
            catalogued().did_you_mean("flyod"),
            Some("Floyd".to_owned()),
            "two letters typed the wrong way round count as one mistake"
        );
        assert_eq!(
            catalogued().did_you_mean("przybylowitz"),
            Some("Przybyłowicz".to_owned()),
            "the marked spelling is what a pane draws"
        );
    }

    #[test]
    fn only_the_word_that_is_wrong_is_corrected() {
        assert_eq!(
            catalogued().did_you_mean("pink floid"),
            Some("pink Floyd".to_owned())
        );
    }

    #[test]
    fn a_word_no_spelling_is_near_is_left_as_it_was() {
        assert_eq!(catalogued().did_you_mean("metallica"), None);
    }

    #[test]
    fn a_word_too_short_to_mistype_is_never_corrected() {
        assert_eq!(catalogued().did_you_mean("sky"), None);
        assert_eq!(catalogued().did_you_mean("ske"), None);
    }

    #[test]
    fn a_scoped_word_is_weighed_against_that_column_alone() {
        assert_eq!(
            catalogued().did_you_mean("artist:ekhoes"),
            None,
            "a title was suggested for an artist"
        );
        assert_eq!(
            catalogued().did_you_mean("title:ekhoes"),
            Some("title:Echoes".to_owned())
        );
    }

    #[test]
    fn a_term_beside_a_word_is_carried_through_as_it_was_read() {
        assert_eq!(
            catalogued().did_you_mean("floid year:1973"),
            Some("Floyd year:1973".to_owned())
        );
    }

    #[test]
    fn a_word_a_search_denies_is_not_corrected() {
        assert_eq!(catalogued().did_you_mean("-floid"), None);
    }

    #[test]
    fn a_phrase_is_corrected_a_word_at_a_time_and_kept_a_phrase() {
        assert_eq!(
            catalogued().did_you_mean("\"dark syde\""),
            Some("\"dark Side\"".to_owned())
        );
    }

    #[test]
    fn a_phrase_mistyped_as_a_whole_is_answered_with_the_whole_name_the_catalog_holds() {
        let spellings = catalogued();

        assert_eq!(
            spellings.did_you_mean("\"the great gig in teh sky\""),
            Some("\"The Great Gig in the Sky\"".to_owned()),
            "a word too short to correct on its own left the whole phrase unanswered"
        );
    }

    #[test]
    fn a_name_mistyped_without_quotes_is_answered_as_the_whole_name_the_catalog_holds() {
        let spellings = catalogued();

        assert_eq!(
            spellings.did_you_mean("the great gig in teh sky"),
            Some("The Great Gig in the Sky".to_owned()),
            "a word too short to correct on its own left the run of tokens unanswered"
        );
    }

    #[test]
    fn a_run_of_tokens_is_weighed_as_a_name_only_where_a_word_in_it_is_beyond_correcting() {
        let spellings = catalogued();

        assert_eq!(
            spellings.did_you_mean("pink floid"),
            Some("pink Floyd".to_owned()),
            "a word the catalog could spell on its own was taken into a whole name"
        );
        assert_eq!(
            spellings.did_you_mean("the great gig in the sky"),
            None,
            "a name the catalog holds was answered with itself"
        );
    }

    #[test]
    fn a_name_is_taken_out_of_the_tokens_around_it_rather_than_the_whole_query() {
        let spellings = catalogued();

        assert_eq!(
            spellings.did_you_mean("the great gig in teh sky metallica"),
            Some("The Great Gig in the Sky metallica".to_owned()),
            "a token beside the name was drawn into it or lost"
        );
    }

    #[test]
    fn a_run_of_tokens_a_term_cuts_is_weighed_no_further_than_the_term() {
        let spellings = catalogued();

        assert_eq!(
            spellings.did_you_mean("the great gig added:7d in teh sky"),
            None,
            "a run was read through a term that stands between its tokens"
        );
    }

    #[test]
    fn a_scoped_run_of_tokens_is_weighed_against_that_column_alone() {
        let spellings = catalogued();

        assert_eq!(
            spellings.did_you_mean("artist:the artist:great artist:gig artist:in artist:teh                                     artist:sky"),
            None,
            "a title was offered for a run scoped to the artist"
        );
    }

    #[test]
    fn a_phrase_the_catalog_holds_as_a_whole_name_is_left_alone() {
        let spellings = catalogued();

        assert_eq!(spellings.did_you_mean("\"the great gig in the sky\""), None);
        assert_eq!(
            spellings.did_you_mean("\"the great gig\""),
            None,
            "a phrase that begins a name the catalog holds was corrected"
        );
    }

    #[test]
    fn a_run_typed_as_one_word_is_split_into_as_many_as_the_catalog_holds() {
        let spellings = catalogued();

        assert_eq!(
            spellings.did_you_mean("thegreatgig"),
            Some("the Great Gig".to_owned()),
            "a run that cuts into three was left as it was"
        );
    }

    #[test]
    fn a_word_spelt_more_than_one_way_is_answered_in_the_marked_spelling() {
        let mut spellings = Spellings::default();
        spellings.taking(Column::Title, "PYRAMID");
        spellings.taking(Column::Title, "Pyramid Song");

        assert_eq!(
            spellings.did_you_mean("pyramd"),
            Some("Pyramid".to_owned()),
            "a shouted spelling was drawn over a written one"
        );

        let mut marked = Spellings::default();
        marked.taking(Column::Artist, "Bjork");
        marked.taking(Column::Artist, "Björk");

        assert_eq!(marked.did_you_mean("bjorc"), Some("Björk".to_owned()));
    }

    #[test]
    fn the_word_the_most_rows_hold_wins_a_tie() {
        let mut spellings = Spellings::default();
        spellings.taking(Column::Title, "Rain");
        spellings.taking(Column::Title, "Rain");
        spellings.taking(Column::Title, "Ruin");

        assert_eq!(spellings.did_you_mean("rein"), Some("Rain".to_owned()));
    }

    #[test]
    fn a_word_run_together_with_the_next_is_answered_as_the_two_the_catalog_holds() {
        let spellings = catalogued();

        assert_eq!(
            spellings.did_you_mean("pinkfloyd"),
            Some("Pink Floyd".to_owned()),
            "a run-together was not split into the two words the catalog holds"
        );
        assert_eq!(
            spellings.did_you_mean("darkside"),
            Some("Dark Side".to_owned())
        );
    }

    #[test]
    fn a_word_typed_in_two_halves_is_answered_as_the_one_the_catalog_holds() {
        let spellings = catalogued();

        assert_eq!(
            spellings.did_you_mean("pink floy d"),
            Some("pink Floyd".to_owned()),
            "two halves of one word were not run together"
        );
        assert_eq!(spellings.did_you_mean("ech oes"), Some("Echoes".to_owned()));
    }

    #[test]
    fn a_scoped_run_together_is_answered_as_a_phrase_so_the_scope_reaches_both_words() {
        let spellings = catalogued();

        assert_eq!(
            spellings.did_you_mean("artist:pinkfloyd"),
            Some("artist:\"Pink Floyd\"".to_owned()),
            "the scope reached only the first word of the correction"
        );
    }

    #[test]
    fn two_words_the_catalog_holds_apart_are_not_run_together() {
        let spellings = catalogued();

        assert_eq!(spellings.did_you_mean("pink floyd"), None);
        assert_eq!(spellings.did_you_mean("dark side"), None);
    }

    #[test]
    fn a_run_together_the_catalog_does_not_hold_both_halves_of_is_left_as_it_was() {
        let spellings = catalogued();

        assert_eq!(
            spellings.did_you_mean("pinkfloid"),
            None,
            "a run-together was answered although the catalog holds no such second word"
        );
    }

    #[test]
    fn a_run_too_short_to_be_corrected_is_too_short_to_be_split() {
        let mut spellings = Spellings::default();
        spellings.taking(Column::Title, "In A Silent Way");

        assert_eq!(
            spellings.did_you_mean("ina"),
            None,
            "a run under the correction budget was split all the same"
        );
        assert_eq!(
            spellings.did_you_mean("silentway"),
            Some("Silent Way".to_owned())
        );
    }

    #[test]
    fn a_query_with_no_word_long_enough_to_mistype_is_not_worth_reading_the_catalog_for() {
        assert!(!worth_asking(&Search::read("sky")));
        assert!(!worth_asking(&Search::read("year:1973")));
        assert!(!worth_asking(&Search::read("")));
        assert!(!worth_asking(&Search::read("-floid")));
        assert!(worth_asking(&Search::read("floid")));
        assert!(worth_asking(&Search::read("sky floid")));
        assert!(
            worth_asking(&Search::read("flo yd")),
            "two runs too short to mistype apart are long enough run together"
        );
    }

    #[test]
    fn a_distance_is_bounded_by_what_the_word_can_afford() {
        assert_eq!(apart_by("floyd", "floyd", 1), Some(0));
        assert_eq!(apart_by("floyd", "floid", 1), Some(1));
        assert_eq!(apart_by("floyd", "flyod", 1), Some(1));
        assert_eq!(apart_by("floyd", "flood", 1), Some(1));
        assert_eq!(apart_by("floyd", "blues", 1), None);
        assert_eq!(apart_by("floyd", "fl", 1), None);
    }
}
