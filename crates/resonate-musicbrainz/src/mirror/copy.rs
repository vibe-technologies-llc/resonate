#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Archive {
    Core,
    Derived,
}

pub struct Table {
    pub name: &'static str,
    pub from: Archive,
    pub columns: &'static [usize],
    pub laid: &'static str,
    pub indexed: &'static [&'static str],
}

pub const NULL: &[u8] = b"\\N";

macro_rules! table {
    ($name:literal, $from:ident, [$($column:literal),*], $laid:literal, [$($index:literal),*]) => {
        Table {
            name: $name,
            from: Archive::$from,
            columns: &[$($column),*],
            laid: $laid,
            indexed: &[$($index),*],
        }
    };
}

pub const TABLES: &[Table] = &[
    table!("area", Core, [0, 2], "id INTEGER, name TEXT", ["id, name"]),
    table!(
        "artist",
        Core,
        [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 16, 17],
        "id INTEGER, gid TEXT, name TEXT, sort_name TEXT, by INTEGER, bm INTEGER, bd INTEGER, \
         ey INTEGER, em INTEGER, ed INTEGER, type INTEGER, area INTEGER, gender INTEGER, \
         comment TEXT, ended TEXT, begin_area INTEGER",
        ["id"]
    ),
    table!(
        "artist_alias",
        Core,
        [1, 2],
        "artist INTEGER, name TEXT",
        ["artist, name"]
    ),
    table!(
        "artist_credit_name",
        Core,
        [0, 1, 2, 3, 4],
        "artist_credit INTEGER, position INTEGER, artist INTEGER, name TEXT, join_phrase TEXT",
        [
            "artist_credit, position, artist, name, join_phrase",
            "artist, artist_credit"
        ]
    ),
    table!("artist_type", Core, [0, 1], "id INTEGER, name TEXT", []),
    table!("gender", Core, [0, 1], "id INTEGER, name TEXT", []),
    table!(
        "iso_3166_1",
        Core,
        [0, 1],
        "area INTEGER, code TEXT",
        ["area, code"]
    ),
    table!(
        "isrc",
        Core,
        [1, 2],
        "recording INTEGER, isrc TEXT",
        ["recording, isrc"]
    ),
    table!(
        "l_artist_url",
        Core,
        [1, 2, 3],
        "link INTEGER, entity0 INTEGER, entity1 INTEGER",
        ["entity0, link, entity1"]
    ),
    table!(
        "l_recording_url",
        Core,
        [1, 2, 3],
        "link INTEGER, entity0 INTEGER, entity1 INTEGER",
        ["entity0, link, entity1"]
    ),
    table!(
        "l_release_group_url",
        Core,
        [1, 2, 3],
        "link INTEGER, entity0 INTEGER, entity1 INTEGER",
        ["entity0, link, entity1"]
    ),
    table!(
        "l_release_url",
        Core,
        [1, 2, 3],
        "link INTEGER, entity0 INTEGER, entity1 INTEGER",
        ["entity0, link, entity1"]
    ),
    table!("label", Core, [0, 2], "id INTEGER, name TEXT", ["id, name"]),
    table!(
        "link",
        Core,
        [0, 1],
        "id INTEGER, link_type INTEGER",
        ["id, link_type"]
    ),
    table!("link_type", Core, [0, 6], "id INTEGER, name TEXT", []),
    table!(
        "medium",
        Core,
        [0, 1, 2, 3, 4, 7],
        "id INTEGER, release INTEGER, position INTEGER, format INTEGER, name TEXT, track_count INTEGER",
        [
            "release, position, id, format, name, track_count",
            "id, release, position"
        ]
    ),
    table!("medium_format", Core, [0, 1], "id INTEGER, name TEXT", []),
    table!(
        "recording",
        Core,
        [0, 1, 2, 3, 4],
        "id INTEGER, gid TEXT, name TEXT, artist_credit INTEGER, length INTEGER",
        ["id, gid, name, artist_credit, length"]
    ),
    table!(
        "release",
        Core,
        [0, 1, 2, 3, 4, 5, 9, 10],
        "id INTEGER, gid TEXT, name TEXT, artist_credit INTEGER, release_group INTEGER, \
         status INTEGER, barcode TEXT, comment TEXT",
        ["release_group, id", "id, artist_credit, barcode, comment"]
    ),
    table!(
        "release_country",
        Core,
        [0, 1, 2, 3, 4],
        "release INTEGER, country INTEGER, y INTEGER, m INTEGER, d INTEGER",
        []
    ),
    table!(
        "release_group",
        Core,
        [0, 1, 2, 3, 4, 5],
        "id INTEGER, gid TEXT, name TEXT, artist_credit INTEGER, type INTEGER, comment TEXT",
        ["artist_credit, id", "id, artist_credit, comment"]
    ),
    table!(
        "release_group_primary_type",
        Core,
        [0, 1],
        "id INTEGER, name TEXT",
        []
    ),
    table!(
        "release_group_secondary_type",
        Core,
        [0, 1],
        "id INTEGER, name TEXT",
        []
    ),
    table!(
        "release_group_secondary_type_join",
        Core,
        [0, 1],
        "release_group INTEGER, secondary_type INTEGER",
        []
    ),
    table!(
        "release_label",
        Core,
        [1, 2, 3],
        "release INTEGER, label INTEGER, catalog_number TEXT",
        ["release, label, catalog_number"]
    ),
    table!("release_status", Core, [0, 1], "id INTEGER, name TEXT", []),
    table!(
        "release_unknown_country",
        Core,
        [0, 1, 2, 3],
        "release INTEGER, y INTEGER, m INTEGER, d INTEGER",
        []
    ),
    table!(
        "track",
        Core,
        [0, 1, 2, 3, 4, 5, 6, 7, 8],
        "id INTEGER, gid TEXT, recording INTEGER, medium INTEGER, position INTEGER, number TEXT, \
         name TEXT, artist_credit INTEGER, length INTEGER",
        ["recording, medium, position, number, name, artist_credit, gid, length"]
    ),
    table!("url", Core, [0, 2], "id INTEGER, url TEXT", ["id, url"]),
    table!(
        "artist_tag",
        Derived,
        [0, 1, 2],
        "artist INTEGER, tag INTEGER, count INTEGER",
        ["artist, tag, count"]
    ),
    table!(
        "release_group_meta",
        Derived,
        [0, 2, 3, 4],
        "id INTEGER, y INTEGER, m INTEGER, d INTEGER",
        []
    ),
    table!(
        "release_meta",
        Derived,
        [0, 4],
        "id INTEGER, cover_art_presence TEXT",
        ["id, cover_art_presence"]
    ),
    table!(
        "tag",
        Derived,
        [0, 1],
        "id INTEGER, name TEXT",
        ["id, name"]
    ),
];

pub fn table_named(entry: &str) -> Option<&'static Table> {
    let name = entry.strip_prefix("mbdump/")?;
    TABLES.iter().find(|table| table.name == name)
}

pub fn fields<'a>(line: &'a [u8], into: &mut Vec<&'a [u8]>) {
    into.clear();
    into.extend(line.split(|byte| *byte == b'\t'));
}

pub fn unescaped(field: &[u8]) -> Option<String> {
    if field == NULL {
        return None;
    }
    if !field.contains(&b'\\') {
        return Some(String::from_utf8_lossy(field).into_owned());
    }
    let mut plain = Vec::with_capacity(field.len());
    let mut bytes = field.iter().copied().peekable();
    while let Some(byte) = bytes.next() {
        if byte != b'\\' {
            plain.push(byte);
            continue;
        }
        let Some(escaped) = bytes.next() else {
            plain.push(b'\\');
            break;
        };
        match escaped {
            b'n' => plain.push(b'\n'),
            b't' => plain.push(b'\t'),
            b'r' => plain.push(b'\r'),
            b'b' => plain.push(0x08),
            b'f' => plain.push(0x0c),
            b'v' => plain.push(0x0b),
            b'0'..=b'7' => {
                let mut value = u32::from(escaped - b'0');
                for _ in 0..2 {
                    match bytes.peek() {
                        Some(digit @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(digit - b'0');
                            bytes.next();
                        }
                        _ => break,
                    }
                }
                plain.push((value & 0xff) as u8);
            }
            b'x' => {
                let mut value = 0_u32;
                let mut digits = 0;
                for _ in 0..2 {
                    match bytes.peek().and_then(|digit| (*digit as char).to_digit(16)) {
                        Some(digit) => {
                            value = value * 16 + digit;
                            digits += 1;
                            bytes.next();
                        }
                        None => break,
                    }
                }
                if digits == 0 {
                    plain.push(b'x');
                } else {
                    plain.push((value & 0xff) as u8);
                }
            }
            other => plain.push(other),
        }
    }
    Some(String::from_utf8_lossy(&plain).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_field_reads_its_escapes_and_its_null() {
        assert_eq!(unescaped(b"\\N"), None);
        assert_eq!(unescaped(b"plain").as_deref(), Some("plain"));
        assert_eq!(unescaped(b"a\\tb\\nc\\\\d").as_deref(), Some("a\tb\nc\\d"));
        assert_eq!(unescaped(b"\\101\\x42").as_deref(), Some("AB"));
        assert_eq!(unescaped(b"").as_deref(), Some(""));
    }

    #[test]
    fn a_copy_field_keeps_what_only_looks_like_a_null_or_an_escape() {
        assert_eq!(unescaped(br"\\N").as_deref(), Some(r"\N"));
        assert_eq!(unescaped(br"\N ").as_deref(), Some("N "));
        assert_eq!(unescaped(br"cut\\").as_deref(), Some(r"cut\"));
        assert_eq!(unescaped(br"cut\").as_deref(), Some(r"cut\"));
        assert_eq!(unescaped(br"\xzz").as_deref(), Some("xzz"));
        assert_eq!(unescaped(br"\x4").as_deref(), Some("\u{4}"));
        assert_eq!(unescaped(br"\1234").as_deref(), Some("S4"));
        assert_eq!(unescaped(br"\.").as_deref(), Some("."));
        assert_eq!(unescaped(br"caf\303\251").as_deref(), Some("caf\u{e9}"));
        assert_eq!(
            unescaped("caf\u{e9} \u{1F3B5}".as_bytes()).as_deref(),
            Some("caf\u{e9} \u{1F3B5}")
        );
    }

    #[test]
    fn a_row_splits_on_tabs_alone_and_keeps_empty_fields() {
        let mut into = Vec::new();

        fields(b"1\t\t\\N\tname with spaces", &mut into);

        assert_eq!(into, [&b"1"[..], b"", br"\N", b"name with spaces"]);

        fields(b"", &mut into);

        assert_eq!(into, [&b""[..]]);
    }

    #[test]
    fn every_table_projects_a_column_for_each_it_lays_out() {
        for table in TABLES {
            assert_eq!(
                table.columns.len(),
                table.laid.split(',').count(),
                "{}",
                table.name
            );
        }
    }
}
