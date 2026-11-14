#[path = "../icon_resources.rs"]
mod icon_resources;

use std::{fs, path::PathBuf};

use icon_resources::{
    Entry, Error, NULL_RECORD, RT_GROUP_ICON, RT_ICON, group_directory, images, joined, records,
};

const WINDOW_ICON: u16 = 1;
const RECORD_HEADER: usize = 32;
const ORDINAL_MARK: u16 = 0xFFFF;
const GROUP_ENTRY: usize = 14;
const GROUP_HEADER: usize = 6;

struct Record<'a> {
    resource_type: u16,
    name: u16,
    data: &'a [u8],
}

fn committed_icon() -> Vec<u8> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packaging/windows/resonate.ico");

    fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn double(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

fn read_records(mut bytes: &[u8]) -> Vec<Record<'_>> {
    let mut read = Vec::new();

    while !bytes.is_empty() {
        let length = usize::try_from(double(bytes, 0)).unwrap();
        let header = usize::try_from(double(bytes, 4)).unwrap();

        assert_eq!(header, RECORD_HEADER);
        assert_eq!(word(bytes, 8), ORDINAL_MARK);
        assert_eq!(word(bytes, 12), ORDINAL_MARK);
        assert_eq!(double(bytes, 16), 0, "a data version");
        assert_eq!(word(bytes, 22), 0, "a neutral language");
        read.push(Record {
            resource_type: word(bytes, 10),
            name: word(bytes, 14),
            data: &bytes[header..header + length],
        });
        bytes = &bytes[(header + length).next_multiple_of(4)..];
    }
    read
}

#[test]
fn every_image_of_the_icon_is_a_numbered_icon_resource_holding_its_bytes() {
    let ico = committed_icon();
    let held = images(&ico).unwrap();
    let resources = records(&ico, WINDOW_ICON).unwrap();
    let read = read_records(&resources);

    let pictures: Vec<&Record<'_>> = read
        .iter()
        .filter(|record| record.resource_type == RT_ICON)
        .collect();

    assert!(held.len() >= 4);
    assert_eq!(pictures.len(), held.len());
    for (index, (picture, image)) in pictures.iter().zip(&held).enumerate() {
        assert_eq!(usize::from(picture.name), index + 1);
        assert_eq!(picture.data, image.bytes);
    }
}

#[test]
fn the_group_the_window_loads_names_each_image_as_the_icon_file_does() {
    let ico = committed_icon();
    let held = images(&ico).unwrap();
    let resources = records(&ico, WINDOW_ICON).unwrap();
    let read = read_records(&resources);

    let groups: Vec<&Record<'_>> = read
        .iter()
        .filter(|record| record.resource_type == RT_GROUP_ICON)
        .collect();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].name, WINDOW_ICON);

    let directory = groups[0].data;
    assert_eq!(word(directory, 0), 0);
    assert_eq!(word(directory, 2), 1);
    assert_eq!(usize::from(word(directory, 4)), held.len());
    assert_eq!(directory.len(), GROUP_HEADER + GROUP_ENTRY * held.len());
    assert_eq!(directory, group_directory(&held).unwrap().as_slice());

    for (index, image) in held.iter().enumerate() {
        let at = GROUP_HEADER + index * GROUP_ENTRY;
        let entry = Entry {
            width: directory[at],
            height: directory[at + 1],
            colours: directory[at + 2],
            reserved: directory[at + 3],
            planes: word(directory, at + 4),
            bits: word(directory, at + 6),
            length: double(directory, at + 8),
            offset: 0,
        };

        assert_eq!(
            Entry {
                offset: 0,
                ..image.entry
            },
            entry,
            "image {index} is described otherwise than the icon file describes it"
        );
        assert_eq!(usize::from(word(directory, at + 12)), index + 1);
        assert_eq!(usize::try_from(entry.length).unwrap(), image.bytes.len());
    }
}

#[test]
fn the_icon_records_go_in_ahead_of_a_resource_file_that_opens_with_the_empty_record() {
    let ico = committed_icon();
    let icons = records(&ico, WINDOW_ICON).unwrap();

    let mut version = NULL_RECORD.to_vec();
    version.extend_from_slice(&[4, 0, 0, 0, 32, 0, 0, 0]);
    version.extend_from_slice(&[0xFF, 0xFF, 16, 0, 0xFF, 0xFF, 1, 0]);
    version.extend_from_slice(&[0; 16]);
    version.extend_from_slice(&[7, 7, 7, 7]);

    let whole = joined(&icons, &version).unwrap();
    let read = read_records(&whole);

    assert_eq!(&whole[..NULL_RECORD.len()], &NULL_RECORD);
    assert_eq!(read[0].resource_type, 0);
    assert_eq!(read.last().unwrap().resource_type, 16);
    assert_eq!(read.last().unwrap().data, &[7, 7, 7, 7]);
    assert_eq!(
        read.iter()
            .filter(|record| record.resource_type == RT_GROUP_ICON)
            .count(),
        1
    );
    assert!(matches!(joined(&icons, &icons), Err(Error::NoEmptyRecord)));
}

#[test]
fn a_file_that_is_no_icon_or_cut_short_is_refused_by_name() {
    let ico = committed_icon();

    assert!(matches!(
        images(&[1, 0, 1, 0, 1, 0]),
        Err(Error::NotAnIcon {
            reserved: 1,
            kind: 1
        })
    ));
    assert!(matches!(images(&ico[..4]), Err(Error::Truncated { .. })));
    assert!(matches!(
        images(&ico[..ico.len() - 1]),
        Err(Error::ImageOutsideFile { .. })
    ));
}
