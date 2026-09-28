use std::path::Path;

use crate::{
    PlaylistEntry, Result,
    sheet::{self, Described, Listed, Sheet, Timed},
};

const HEADER: &str = "#EXTM3U";

const NAME_TAG: &str = "#PLAYLIST:";

const ENTRY_TAG: &str = "#EXTINF:";

const COMMENT: char = '#';

const UNKNOWN_LENGTH: &str = "-1";

const VLC_OPTION_TAG: &str = "#EXTVLCOPT:";

pub fn read(text: &str, beside: &Path) -> Sheet {
    let mut sheet = Sheet::default();
    let mut timed = Timed::default();

    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(option) = line.strip_prefix(VLC_OPTION_TAG) {
            timed.read(option);
            continue;
        }
        if let Some(declared) = line.strip_prefix(NAME_TAG) {
            let declared = declared.trim();
            if !declared.is_empty() {
                sheet.declared = Some(declared.to_owned());
            }
            continue;
        }
        if line.starts_with(COMMENT) {
            continue;
        }
        match sheet::located(line, beside) {
            Some(location) => sheet.locations.push(Listed {
                location,
                timed: std::mem::take(&mut timed),
            }),
            None => {
                timed = Timed::default();
                sheet.elsewhere += 1;
            }
        }
    }

    sheet
}

pub fn write(name: &str, entries: &[PlaylistEntry], beside: &Path) -> Result<String> {
    let mut text = format!("{HEADER}\n{NAME_TAG}{}\n", sheet::one_line(name));

    for entry in entries {
        let Some(file) = entry.location().as_path() else {
            continue;
        };

        text.push_str(ENTRY_TAG);
        text.push_str(&sheet::one_line(&extinf(&sheet::describe(entry))));
        text.push('\n');
        if let Some(timed) = sheet::timed(entry) {
            for (option, at) in [
                (sheet::START_TIME, timed.from),
                (sheet::STOP_TIME, timed.to),
            ] {
                if let Some(at) = at {
                    text.push_str(VLC_OPTION_TAG);
                    text.push_str(option);
                    text.push_str(&sheet::written_seconds(at));
                    text.push('\n');
                }
            }
        }
        text.push_str(&sheet::as_a_row(file, beside)?);
        text.push('\n');
    }

    Ok(text)
}

fn extinf(described: &Described) -> String {
    let seconds = described
        .seconds
        .map_or_else(|| UNKNOWN_LENGTH.to_owned(), |seconds| seconds.to_string());

    match described.artist.as_deref() {
        Some(artist) => format!("{seconds},{artist} - {}", described.title),
        None => format!("{seconds},{}", described.title),
    }
}
