use std::{borrow::Cow, path::Path};

use crate::organise::without_padding;

const MAX_PATH_UNITS: usize = 260;
const TERMINATING_NUL: usize = 1;
const ROOM_FOR_DERIVED_NAMES: usize = 48;
const SEPARATOR_UNITS: usize = 1;
const VERBATIM_PREFIX: &str = r"\\?\";
const VERBATIM_NETWORK_PREFIX: &str = r"\\?\UNC\";
const NETWORK_PREFIX: &str = r"\\";
const SEPARATORS: [char; 2] = ['\\', '/'];

pub(crate) const LEGACY_UNITS_AT_MOST: usize =
    MAX_PATH_UNITS - TERMINATING_NUL - ROOM_FOR_DERIVED_NAMES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Reach {
    root_units: usize,
    units_at_most: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Part {
    name: String,
    kept: String,
}

impl Part {
    pub(crate) fn new(name: String, kept: String) -> Self {
        Self { name, kept }
    }

    pub(crate) fn whole(&self) -> String {
        format!("{}{}", self.name, self.kept)
    }

    fn can_lose_a_letter(&self) -> bool {
        self.name.chars().nth(1).is_some()
    }

    fn loses_a_letter(&mut self) {
        self.name.pop();
        let kept = without_padding(&self.name).len();
        self.name.truncate(kept);
    }
}

impl Reach {
    pub(crate) const UNLIMITED: Self = Self {
        root_units: 0,
        units_at_most: usize::MAX,
    };

    pub(crate) fn of(root: &Path) -> Self {
        if cfg!(windows) {
            Self::legacy(root)
        } else {
            Self::UNLIMITED
        }
    }

    pub(crate) fn legacy(root: &Path) -> Self {
        Self {
            root_units: units_of(&root.to_string_lossy()),
            units_at_most: LEGACY_UNITS_AT_MOST,
        }
    }

    pub(crate) fn weighs(&self, parts: &[Part], weigh: impl Fn(&Part) -> usize) -> usize {
        self.root_units
            + parts
                .iter()
                .map(|part| SEPARATOR_UNITS + weigh(part))
                .sum::<usize>()
    }

    pub(crate) fn admits(&self, relative: &Path) -> bool {
        let below = relative
            .components()
            .map(|component| SEPARATOR_UNITS + units_of(&component.as_os_str().to_string_lossy()))
            .sum::<usize>();
        self.root_units + below <= self.units_at_most
    }

    pub(crate) fn fit(&self, parts: &mut [Part], weigh: impl Fn(&Part) -> usize) -> bool {
        while self.weighs(parts, &weigh) > self.units_at_most {
            let longest = parts
                .iter_mut()
                .filter(|part| part.can_lose_a_letter())
                .max_by_key(|part| weigh(part));
            match longest {
                Some(part) => part.loses_a_letter(),
                None => return false,
            }
        }
        true
    }
}

pub(crate) fn units_of(text: &str) -> usize {
    plain(text)
        .trim_end_matches(SEPARATORS)
        .encode_utf16()
        .count()
}

fn plain(text: &str) -> Cow<'_, str> {
    if let Some(network) = text.strip_prefix(VERBATIM_NETWORK_PREFIX) {
        return Cow::Owned(format!("{NETWORK_PREFIX}{network}"));
    }
    Cow::Borrowed(text.strip_prefix(VERBATIM_PREFIX).unwrap_or(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn weighed(part: &Part) -> usize {
        part.whole().encode_utf16().count()
    }

    fn parts(names: &[(&str, &str)]) -> Vec<Part> {
        names
            .iter()
            .map(|(name, kept)| Part::new((*name).to_owned(), (*kept).to_owned()))
            .collect()
    }

    fn whole_path(root: &str, parts: &[Part]) -> String {
        let mut path = root.trim_end_matches(SEPARATORS).to_owned();
        for part in parts {
            path.push('\\');
            path.push_str(&part.whole());
        }
        path
    }

    #[test]
    fn a_root_is_weighed_in_utf16_units_without_a_verbatim_prefix_or_a_trailing_separator() {
        assert_eq!(units_of(r"C:\Music"), 8);
        assert_eq!(units_of(r"C:\Music\"), 8);
        assert_eq!(units_of(r"\\?\C:\Music"), 8);
        assert_eq!(
            units_of(r"\\?\UNC\server\share\Music"),
            r"\\server\share\Music".len()
        );
        assert_eq!(units_of("C:\\\u{1d11e}"), 5);
        assert_eq!(units_of("C:\\\u{e9}"), 4);
    }

    #[test]
    fn a_path_that_fits_is_left_whole() {
        let reach = Reach::legacy(Path::new(r"C:\Music"));
        let mut kept = parts(&[("Artist", ""), ("Album", ""), ("01 Title", ".flac")]);
        let before = kept.clone();

        assert!(reach.fit(&mut kept, weighed));

        assert_eq!(kept, before);
    }

    #[test]
    fn a_path_past_the_limit_is_cut_until_the_whole_fits_and_the_extension_stays() {
        let root = r"C:\Users\someone\Music";
        let reach = Reach::legacy(Path::new(root));
        let mut kept = parts(&[
            (&"a".repeat(120), ""),
            (&"b".repeat(120), ""),
            (&"c".repeat(120), ".flac"),
        ]);

        assert!(reach.fit(&mut kept, weighed));

        let written = whole_path(root, &kept);
        assert!(
            written.encode_utf16().count() <= LEGACY_UNITS_AT_MOST,
            "{written}"
        );
        assert!(kept[2].whole().ends_with(".flac"));
        assert!(kept.iter().all(|part| !part.name.is_empty()));
    }

    #[test]
    fn the_longest_name_loses_letters_first_so_the_cut_is_shared() {
        let root = r"C:\Music";
        let reach = Reach::legacy(Path::new(root));
        let mut kept = parts(&[
            (&"a".repeat(200), ""),
            ("short", ""),
            (&"c".repeat(100), ".flac"),
        ]);

        assert!(reach.fit(&mut kept, weighed));

        assert_eq!(kept[1].name, "short");
        assert!(kept[0].name.len() < 200);
        assert!(kept[2].name.len() < 100);
        let written = whole_path(root, &kept);
        assert_eq!(
            written.encode_utf16().count(),
            LEGACY_UNITS_AT_MOST,
            "{written}"
        );
    }

    #[test]
    fn a_cut_never_ends_on_padding_or_splits_a_letter() {
        let root = r"C:\Music";
        let reach = Reach::legacy(Path::new(root));
        let mut kept = parts(&[(&format!("{} . x", "\u{1d11e}".repeat(100)), ".flac")]);
        kept.push(Part::new("b".repeat(150), String::new()));

        assert!(reach.fit(&mut kept, weighed));

        for part in &kept {
            assert_eq!(part.name, without_padding(&part.name));
        }
        assert!(whole_path(root, &kept).encode_utf16().count() <= LEGACY_UNITS_AT_MOST);
    }

    #[test]
    fn a_root_so_deep_that_one_letter_a_folder_does_not_fit_is_out_of_reach() {
        let root = format!(r"C:\{}", "r".repeat(LEGACY_UNITS_AT_MOST));
        let reach = Reach::legacy(Path::new(&root));
        let mut kept = parts(&[("Artist", ""), ("01 Title", ".flac")]);

        assert!(!reach.fit(&mut kept, weighed));
    }

    #[test]
    fn a_relative_path_is_admitted_while_the_root_and_it_fit_together() {
        let root = r"C:\Music";
        let reach = Reach::legacy(Path::new(root));
        let room = LEGACY_UNITS_AT_MOST - units_of(root) - SEPARATOR_UNITS;

        assert!(reach.admits(Path::new(&"a".repeat(room))));
        assert!(!reach.admits(Path::new(&"a".repeat(room + 1))));
    }

    #[test]
    fn nothing_is_cut_where_there_is_no_limit() {
        let mut kept = parts(&[(&"a".repeat(255), ""), (&"b".repeat(255), ".flac")]);
        let before = kept.clone();

        assert!(Reach::UNLIMITED.fit(&mut kept, weighed));

        assert_eq!(kept, before);
        assert_eq!(
            Reach::of(Path::new("/home/me/Music")),
            if cfg!(windows) {
                Reach::legacy(Path::new("/home/me/Music"))
            } else {
                Reach::UNLIMITED
            }
        );
    }

    #[test]
    fn the_room_left_for_derived_names_covers_the_longest_of_them() {
        let longest_staged =
            ".".len() + ".".len() + 10 + "-".len() + 10 + ".resonate-delivery".len();
        let parked = ".resonate-parked-".len() + 10;

        assert!(ROOM_FOR_DERIVED_NAMES >= longest_staged);
        assert!(ROOM_FOR_DERIVED_NAMES >= parked);
    }
}
