use std::path::Path;

use resonate_core::MediaLocation;

use crate::{
    PlaylistEntry, Result,
    sheet::{self, Described, Sheet},
};

pub const HEADER: &str = "[playlist]";

const FILE_KEY: &str = "File";

const TITLE_KEY: &str = "Title";

const LENGTH_KEY: &str = "Length";

const NAME_KEY: &str = "X-GNOME-Title";

const COUNT_KEY: &str = "NumberOfEntries";

const VERSION_KEY: &str = "Version";

const PLS_VERSION: u32 = 2;

const UNKNOWN_LENGTH: &str = "-1";

pub fn read(text: &str, beside: &Path) -> Sheet {
    let mut sheet = Sheet::default();
    let mut numbered: Vec<(u32, MediaLocation)> = Vec::new();

    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());

        if key.eq_ignore_ascii_case(NAME_KEY) {
            if !value.is_empty() {
                sheet.declared = Some(value.to_owned());
            }
            continue;
        }
        if key.eq_ignore_ascii_case(COUNT_KEY) {
            sheet.promised = value.parse().ok();
            continue;
        }
        let Some(at) = numbering(key, FILE_KEY) else {
            continue;
        };
        match sheet::located(value, beside) {
            Some(location) => numbered.push((at, location)),
            None => sheet.elsewhere += 1,
        }
    }

    numbered.sort_by_key(|(at, _)| *at);
    sheet.locations = numbered.into_iter().map(|(_, location)| location).collect();
    sheet
}

pub fn write(name: &str, entries: &[PlaylistEntry], beside: &Path) -> Result<String> {
    let mut text = format!("{HEADER}\n{NAME_KEY}={}\n", sheet::one_line(name));
    let mut written = 0;

    for entry in entries {
        let Some(file) = entry.location().as_path() else {
            continue;
        };
        let described = sheet::describe(entry);
        written += 1;

        text.push_str(&format!(
            "{FILE_KEY}{written}={}\n",
            sheet::as_a_row(file, beside)?
        ));
        text.push_str(&format!(
            "{TITLE_KEY}{written}={}\n",
            sheet::one_line(&titled(&described))
        ));
        text.push_str(&format!(
            "{LENGTH_KEY}{written}={}\n",
            described
                .seconds
                .map_or_else(|| UNKNOWN_LENGTH.to_owned(), |seconds| seconds.to_string())
        ));
    }
    text.push_str(&format!(
        "{COUNT_KEY}={written}\n{VERSION_KEY}={PLS_VERSION}\n"
    ));

    Ok(text)
}

fn numbering(key: &str, named: &str) -> Option<u32> {
    let at = key.get(..named.len())?;
    at.eq_ignore_ascii_case(named)
        .then(|| key[named.len()..].parse().ok())
        .flatten()
}

fn titled(described: &Described) -> String {
    match described.artist.as_deref() {
        Some(artist) => format!("{artist} - {}", described.title),
        None => described.title.clone(),
    }
}
