use unicode_width::UnicodeWidthStr as _;

pub const TURNS_THE_READING: [char; 9] = [
    '\u{202a}', '\u{202b}', '\u{202c}', '\u{202d}', '\u{202e}', '\u{2066}', '\u{2067}', '\u{2068}',
    '\u{2069}',
];

pub fn on_one_line(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() || TURNS_THE_READING.contains(&character) {
                ' '
            } else {
                character
            }
        })
        .collect()
}

pub struct Table {
    headers: Vec<&'static str>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(headers: Vec<&'static str>) -> Self {
        Self {
            headers,
            rows: Vec::new(),
        }
    }

    pub fn push(&mut self, row: Vec<String>) {
        self.rows
            .push(row.iter().map(|cell| on_one_line(cell)).collect());
    }

    fn widths(&self) -> Vec<usize> {
        let mut widths: Vec<usize> = self.headers.iter().map(|header| header.width()).collect();

        for row in &self.rows {
            for (column, cell) in row.iter().enumerate() {
                let width = cell.width();
                match widths.get_mut(column) {
                    Some(slot) => *slot = (*slot).max(width),
                    None => widths.push(width),
                }
            }
        }
        widths
    }

    pub fn render(&self) -> String {
        const GAP: &str = "  ";

        let widths = self.widths();
        let mut out = String::new();
        let line = |out: &mut String, cells: &mut dyn Iterator<Item = &str>| {
            let mut rendered = String::new();
            for (column, cell) in cells.enumerate() {
                if column > 0 {
                    rendered.push_str(GAP);
                }
                rendered.push_str(cell);
                let width = widths.get(column).copied().unwrap_or_default();
                for _ in cell.width()..width {
                    rendered.push(' ');
                }
            }
            out.push_str(rendered.trim_end());
            out.push('\n');
        };

        line(&mut out, &mut self.headers.iter().copied());
        for row in &self.rows {
            line(&mut out, &mut row.iter().map(String::as_str));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Table {
        let mut table = Table::new(vec!["NAME", "RATE"]);
        table.push(vec!["auto_null".to_owned(), "48 kHz".to_owned()]);
        table.push(vec!["a".to_owned(), "44.1 kHz".to_owned()]);
        table
    }

    #[test]
    fn a_cell_carrying_a_control_or_a_reordering_mark_is_laid_on_one_plain_line() {
        let mut table = Table::new(vec!["TITLE", "ARTIST"]);
        table.push(vec![
            "Echoes\x1b]0;owned\x07".to_owned(),
            "Pink\nFloyd  forged\u{202e}".to_owned(),
        ]);

        let rendered = table.render();

        assert_eq!(rendered.lines().count(), 2, "{rendered:?}");
        assert!(
            !rendered
                .chars()
                .any(|character| character != '\n' && character.is_control()),
            "{rendered:?}"
        );
        assert!(!rendered.contains('\u{202e}'));
        assert!(rendered.contains("Echoes ]0;owned "));
    }

    #[test]
    fn every_column_is_as_wide_as_its_widest_cell() {
        assert_eq!(
            table().render(),
            "NAME       RATE\nauto_null  48 kHz\na          44.1 kHz\n"
        );
    }

    #[test]
    fn a_header_wider_than_its_column_sets_the_width() {
        let mut table = Table::new(vec!["NODE NAME", "RATE"]);
        table.push(vec!["a".to_owned(), "48 kHz".to_owned()]);
        assert_eq!(table.render(), "NODE NAME  RATE\na          48 kHz\n");
    }

    #[test]
    fn trailing_padding_is_never_written() {
        let mut table = Table::new(vec!["A", "B"]);
        table.push(vec!["wide".to_owned(), String::new()]);
        table.push(vec!["x".to_owned(), "y".to_owned()]);

        for row in table.render().lines() {
            assert_eq!(row, row.trim_end(), "{row:?} carries trailing padding");
        }
    }

    #[test]
    fn a_row_wider_than_the_header_still_renders_every_cell() {
        let mut table = Table::new(vec!["A"]);
        table.push(vec!["one".to_owned(), "two".to_owned()]);
        assert_eq!(table.render(), "A\none  two\n");
    }

    #[test]
    fn width_is_measured_as_the_terminal_draws_it_rather_than_in_bytes() {
        let mut table = Table::new(vec!["A", "B"]);
        table.push(vec!["Café".to_owned(), "x".to_owned()]);
        table.push(vec!["abcde".to_owned(), "y".to_owned()]);
        assert_eq!(table.render(), "A      B\nCafé   x\nabcde  y\n");
    }

    #[test]
    fn a_combining_mark_takes_no_room_of_its_own() {
        let mut table = Table::new(vec!["A", "B"]);
        table.push(vec!["Cafe\u{301}".to_owned(), "x".to_owned()]);
        table.push(vec!["abcde".to_owned(), "y".to_owned()]);
        assert_eq!(table.render(), "A      B\nCafe\u{301}   x\nabcde  y\n");
    }

    #[test]
    fn a_double_width_name_takes_two_columns_a_character() {
        let mut table = Table::new(vec!["A", "B"]);
        table.push(vec!["東京".to_owned(), "x".to_owned()]);
        table.push(vec!["abcde".to_owned(), "y".to_owned()]);
        assert_eq!(table.render(), "A      B\n東京   x\nabcde  y\n");
    }
}
