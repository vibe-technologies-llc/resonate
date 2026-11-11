use rusqlite::types::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Term<'a> {
    pub(crate) expression: &'a str,
    pub(crate) falling: bool,
}

const FALLING: &str = " DESC";

pub(crate) fn terms(order: &str) -> Vec<Term<'_>> {
    let mut terms = Vec::new();
    let mut depth = 0_usize;
    let mut began = 0;
    for (at, letter) in order.char_indices() {
        match letter {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                terms.push(term(&order[began..at]));
                began = at + 1;
            }
            _ => {}
        }
    }
    terms.push(term(&order[began..]));
    terms
}

fn term(written: &str) -> Term<'_> {
    let written = written.trim();
    match written.strip_suffix(FALLING) {
        Some(expression) => Term {
            expression: expression.trim_end(),
            falling: true,
        },
        None => Term {
            expression: written,
            falling: false,
        },
    }
}

pub(crate) fn key_read(terms: &[Term<'_>]) -> String {
    let read: Vec<&str> = terms.iter().map(|term| term.expression).collect();

    format!("SELECT {} FROM tracks WHERE tracks.id = ?", read.join(", "))
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Seek {
    pub(crate) conditions: Vec<String>,
    pub(crate) binds: Vec<Value>,
    pub(crate) order: String,
}

impl Seek {
    fn holding(&mut self, condition: String, bind: Option<&Value>) {
        self.conditions.push(condition);
        self.binds.extend(bind.cloned());
    }

    fn tied_on(terms: &[Term<'_>], key: &[Value], untied: &[Term<'_>]) -> Self {
        let mut seek = Self {
            order: ordered(untied),
            ..Self::default()
        };
        for (term, held) in terms.iter().zip(key) {
            match held {
                Value::Null => seek.holding(format!("({}) IS NULL", term.expression), None),
                held => seek.holding(format!("({}) = ?", term.expression), Some(held)),
            }
        }
        seek
    }
}

pub(crate) fn seeks(terms: &[Term<'_>], key: &[Value]) -> Vec<Seek> {
    let Some(((own_term, rest), (own_key, rest_key))) = terms.split_last().zip(key.split_last())
    else {
        return Vec::new();
    };
    let mut seeks = Vec::with_capacity(terms.len() + 1);

    let mut at_the_row = Seek::tied_on(rest, rest_key, &terms[rest.len()..]);
    let from_the_row = match own_term.falling {
        true => "<=",
        false => ">=",
    };
    at_the_row.holding(
        format!("({}) {from_the_row} ?", own_term.expression),
        Some(own_key),
    );
    seeks.push(at_the_row);

    for at in (0..rest.len()).rev() {
        let (term, held) = (rest[at], &rest_key[at]);
        for past in past(term, held) {
            let untied = &terms[at + usize::from(past.ties)..];
            let mut seek = Seek::tied_on(&rest[..at], &rest_key[..at], untied);
            seek.holding(past.condition, past.bind);
            seeks.push(seek);
        }
    }
    seeks
}

fn ordered(terms: &[Term<'_>]) -> String {
    terms
        .iter()
        .map(|term| match term.falling {
            true => format!("{}{FALLING}", term.expression),
            false => term.expression.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

struct Past<'a> {
    condition: String,
    bind: Option<&'a Value>,
    ties: bool,
}

fn past<'a>(term: Term<'_>, held: &'a Value) -> Vec<Past<'a>> {
    let expression = term.expression;
    match (term.falling, held) {
        (false, Value::Null) => vec![Past {
            condition: format!("({expression}) IS NOT NULL"),
            bind: None,
            ties: false,
        }],
        (false, held) => vec![Past {
            condition: format!("({expression}) > ?"),
            bind: Some(held),
            ties: false,
        }],
        (true, Value::Null) => Vec::new(),
        (true, held) => vec![
            Past {
                condition: format!("({expression}) < ?"),
                bind: Some(held),
                ties: false,
            },
            Past {
                condition: format!("({expression}) IS NULL"),
                bind: None,
                ties: true,
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::types::Value;

    use super::{Seek, Term, key_read, seeks, terms};

    #[test]
    fn an_order_is_read_term_by_term_past_the_commas_inside_its_brackets() {
        assert_eq!(
            terms(
                "tracks.codec IN (1, 2, 5) DESC, tracks.title_filed COLLATE NOCASE, tracks.id DESC"
            ),
            vec![
                Term {
                    expression: "tracks.codec IN (1, 2, 5)",
                    falling: true,
                },
                Term {
                    expression: "tracks.title_filed COLLATE NOCASE",
                    falling: false,
                },
                Term {
                    expression: "tracks.id",
                    falling: true,
                },
            ]
        );
        assert_eq!(
            key_read(&terms("tracks.added, tracks.id DESC")),
            "SELECT tracks.added, tracks.id FROM tracks WHERE tracks.id = ?"
        );
    }

    #[test]
    fn the_rows_after_a_key_are_sought_from_the_row_itself_outwards_and_nulls_where_they_sort() {
        let order = terms("tracks.album_id, tracks.played DESC, tracks.id");
        let key = [Value::Integer(4), Value::Integer(9), Value::Integer(17)];
        let sought = |conditions: &[&str], binds: &[i64], order: &str| Seek {
            conditions: conditions
                .iter()
                .map(|condition| (*condition).to_owned())
                .collect(),
            binds: binds.iter().copied().map(Value::Integer).collect(),
            order: order.to_owned(),
        };

        assert_eq!(
            seeks(&order, &key),
            vec![
                sought(
                    &[
                        "(tracks.album_id) = ?",
                        "(tracks.played) = ?",
                        "(tracks.id) >= ?"
                    ],
                    &[4, 9, 17],
                    "tracks.id",
                ),
                sought(
                    &["(tracks.album_id) = ?", "(tracks.played) < ?"],
                    &[4, 9],
                    "tracks.played DESC, tracks.id",
                ),
                sought(
                    &["(tracks.album_id) = ?", "(tracks.played) IS NULL"],
                    &[4],
                    "tracks.id",
                ),
                sought(
                    &["(tracks.album_id) > ?"],
                    &[4],
                    "tracks.album_id, tracks.played DESC, tracks.id",
                ),
            ]
        );

        let unkeyed = [Value::Null, Value::Null, Value::Integer(17)];
        assert_eq!(
            seeks(&order, &unkeyed),
            vec![
                sought(
                    &[
                        "(tracks.album_id) IS NULL",
                        "(tracks.played) IS NULL",
                        "(tracks.id) >= ?"
                    ],
                    &[17],
                    "tracks.id",
                ),
                sought(
                    &["(tracks.album_id) IS NOT NULL"],
                    &[],
                    "tracks.album_id, tracks.played DESC, tracks.id",
                ),
            ]
        );
    }
}
