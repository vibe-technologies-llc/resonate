use std::{
    collections::BTreeSet,
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

use resonate_core::{Mbid, Recording};
use resonate_sync::{
    Bound, Change, DEVICE_BYTES, DeviceId, DeviceNote, Error, FavouriteNote, Held, Listen,
    Malformed, Platform, SongTags, SpaceId, Stamp, Token,
    wire::{
        FromPeer, FromServer, Record, Refusal, SpaceHead, ToPeer, ToServer, decoded_bundle,
        encoded_bundle,
    },
};

#[derive(Clone, Copy, Debug)]
enum Kind {
    ToServer,
    FromServer,
    ToPeer,
    FromPeer,
    Bundle,
}

struct Seed {
    name: &'static str,
    kind: Kind,
    fingerprint: u64,
}

const SEEDS: &[Seed] = &[
    Seed {
        name: "append",
        kind: Kind::ToServer,
        fingerprint: 0x678c_1017_6f16_7ada,
    },
    Seed {
        name: "appended",
        kind: Kind::FromServer,
        fingerprint: 0x0835_ec07_b4ee_4fb0,
    },
    Seed {
        name: "arrived",
        kind: Kind::FromServer,
        fingerprint: 0x7d83_feb9_e0bd_db13,
    },
    Seed {
        name: "bundle",
        kind: Kind::Bundle,
        fingerprint: 0xecc9_9aad_31d0_ee9f,
    },
    Seed {
        name: "bye",
        kind: Kind::ToServer,
        fingerprint: 0xaf63_b94c_8601_b113,
    },
    Seed {
        name: "changes",
        kind: Kind::ToPeer,
        fingerprint: 0xd545_2dda_3085_e538,
    },
    Seed {
        name: "changes-whole",
        kind: Kind::ToPeer,
        fingerprint: 0x98b8_e086_ee40_c15b,
    },
    Seed {
        name: "close",
        kind: Kind::ToServer,
        fingerprint: 0xb99a_137a_565b_8ec2,
    },
    Seed {
        name: "closed",
        kind: Kind::FromServer,
        fingerprint: 0x6f05_5451_01c5_6aa6,
    },
    Seed {
        name: "compact",
        kind: Kind::ToServer,
        fingerprint: 0xc86e_311c_a923_b302,
    },
    Seed {
        name: "compact-staged",
        kind: Kind::ToServer,
        fingerprint: 0xadd6_9e7c_a905_d2ca,
    },
    Seed {
        name: "compacted",
        kind: Kind::FromServer,
        fingerprint: 0x0828_5107_b4e2_bdf3,
    },
    Seed {
        name: "consult",
        kind: Kind::ToPeer,
        fingerprint: 0x0c3c_238a_6569_946e,
    },
    Seed {
        name: "covered",
        kind: Kind::FromPeer,
        fingerprint: 0xd0ab_2fac_1a6d_8844,
    },
    Seed {
        name: "covers",
        kind: Kind::ToPeer,
        fingerprint: 0x777c_cf27_40a9_63c3,
    },
    Seed {
        name: "covers-end",
        kind: Kind::FromPeer,
        fingerprint: 0xaf63_c54c_8601_c577,
    },
    Seed {
        name: "export",
        kind: Kind::ToPeer,
        fingerprint: 0x393a_9487_00ae_ef30,
    },
    Seed {
        name: "exporting",
        kind: Kind::FromPeer,
        fingerprint: 0x8e42_1a02_7d16_28b4,
    },
    Seed {
        name: "fetch",
        kind: Kind::ToPeer,
        fingerprint: 0x04e1_59dc_bd45_ab37,
    },
    Seed {
        name: "fetch-revised",
        kind: Kind::ToPeer,
        fingerprint: 0x2ffd_33e8_c62b_45f9,
    },
    Seed {
        name: "heads",
        kind: Kind::FromServer,
        fingerprint: 0xcceb_9ce8_44fb_28b7,
    },
    Seed {
        name: "heads-asked",
        kind: Kind::ToServer,
        fingerprint: 0x5469_6afd_cbd4_1b5a,
    },
    Seed {
        name: "held",
        kind: Kind::FromPeer,
        fingerprint: 0x7188_8443_572b_70a6,
    },
    Seed {
        name: "held-end",
        kind: Kind::FromPeer,
        fingerprint: 0xaf63_bc4c_8601_b62c,
    },
    Seed {
        name: "hello",
        kind: Kind::ToServer,
        fingerprint: 0x936d_fc7c_5ea8_dbee,
    },
    Seed {
        name: "hello-incarnated",
        kind: Kind::ToServer,
        fingerprint: 0x1899_eead_3df8_59c5,
    },
    Seed {
        name: "holdings",
        kind: Kind::ToPeer,
        fingerprint: 0xaf63_bd4c_8601_b7df,
    },
    Seed {
        name: "kept",
        kind: Kind::ToPeer,
        fingerprint: 0x7459_9ce3_2bc8_fc29,
    },
    Seed {
        name: "kept-end",
        kind: Kind::FromPeer,
        fingerprint: 0x086f_9e07_b51f_436d,
    },
    Seed {
        name: "left",
        kind: Kind::FromServer,
        fingerprint: 0x164a_9cc1_d7d2_4f40,
    },
    Seed {
        name: "lyricsfiles-end",
        kind: Kind::FromPeer,
        fingerprint: 0xaf63_c74c_8601_c8dd,
    },
    Seed {
        name: "meet",
        kind: Kind::ToPeer,
        fingerprint: 0x15d2_af78_91a7_09fc,
    },
    Seed {
        name: "met",
        kind: Kind::FromPeer,
        fingerprint: 0xb7a6_8456_6abe_4d93,
    },
    Seed {
        name: "missing",
        kind: Kind::FromPeer,
        fingerprint: 0xaf63_be4c_8601_b992,
    },
    Seed {
        name: "printed",
        kind: Kind::FromPeer,
        fingerprint: 0x4d25_ada4_a132_b940,
    },
    Seed {
        name: "prints",
        kind: Kind::ToPeer,
        fingerprint: 0xd02d_2552_1881_00f0,
    },
    Seed {
        name: "prints-end",
        kind: Kind::FromPeer,
        fingerprint: 0xaf63_c14c_8601_beab,
    },
    Seed {
        name: "punch",
        kind: Kind::ToServer,
        fingerprint: 0x2e82_4517_c607_2c26,
    },
    Seed {
        name: "punched",
        kind: Kind::FromServer,
        fingerprint: 0x40fb_21ce_ba17_eee3,
    },
    Seed {
        name: "records-sealed",
        kind: Kind::FromServer,
        fingerprint: 0x85be_18ff_eba5_63d7,
    },
    Seed {
        name: "records-tombstoned",
        kind: Kind::FromServer,
        fingerprint: 0x51d8_bb38_5da8_5b2f,
    },
    Seed {
        name: "refused",
        kind: Kind::FromServer,
        fingerprint: 0x084d_b507_b502_841c,
    },
    Seed {
        name: "refused-stale",
        kind: Kind::FromServer,
        fingerprint: 0x084d_b207_b502_7f03,
    },
    Seed {
        name: "relay",
        kind: Kind::ToServer,
        fingerprint: 0x0110_0d00_2eab_9604,
    },
    Seed {
        name: "relayed",
        kind: Kind::FromServer,
        fingerprint: 0x0d01_b02d_22b3_aa3c,
    },
    Seed {
        name: "relaying",
        kind: Kind::ToServer,
        fingerprint: 0xaf63_b84c_8601_af60,
    },
    Seed {
        name: "room",
        kind: Kind::ToServer,
        fingerprint: 0x87a3_21d4_d38c_ab70,
    },
    Seed {
        name: "room-arrived",
        kind: Kind::FromServer,
        fingerprint: 0xcc0f_2c0a_ab30_6e11,
    },
    Seed {
        name: "room-ended",
        kind: Kind::FromServer,
        fingerprint: 0xaf63_cc4c_8601_d15c,
    },
    Seed {
        name: "room-heard",
        kind: Kind::FromServer,
        fingerprint: 0x1b77_659d_d38e_30c8,
    },
    Seed {
        name: "room-kick",
        kind: Kind::ToServer,
        fingerprint: 0x1084_1cc7_9445_4c11,
    },
    Seed {
        name: "room-left",
        kind: Kind::FromServer,
        fingerprint: 0xadfc_2411_767e_e25e,
    },
    Seed {
        name: "room-ping",
        kind: Kind::ToServer,
        fingerprint: 0x083c_c107_b4f4_254d,
    },
    Seed {
        name: "room-pong",
        kind: Kind::FromServer,
        fingerprint: 0x0f39_2b04_de02_0483,
    },
    Seed {
        name: "room-say",
        kind: Kind::ToServer,
        fingerprint: 0xeda0_8c0f_0870_f24a,
    },
    Seed {
        name: "room-say-all",
        kind: Kind::ToServer,
        fingerprint: 0x7709_dbb5_f341_0da3,
    },
    Seed {
        name: "room-welcome",
        kind: Kind::FromServer,
        fingerprint: 0x7619_b168_54d5_aecb,
    },
    Seed {
        name: "sending",
        kind: Kind::FromPeer,
        fingerprint: 0x702f_2b0e_916e_028b,
    },
    Seed {
        name: "sending-revised",
        kind: Kind::FromPeer,
        fingerprint: 0x89a4_0297_2914_a7b2,
    },
    Seed {
        name: "share",
        kind: Kind::ToServer,
        fingerprint: 0xd181_74ef_55c8_384c,
    },
    Seed {
        name: "snapshot",
        kind: Kind::FromServer,
        fingerprint: 0xa8f8_f449_956b_948a,
    },
    Seed {
        name: "stage",
        kind: Kind::ToServer,
        fingerprint: 0xeaa0_0818_75df_2d0d,
    },
    Seed {
        name: "staged",
        kind: Kind::ToServer,
        fingerprint: 0x7f77_deac_b56f_b2af,
    },
    Seed {
        name: "staging",
        kind: Kind::FromServer,
        fingerprint: 0xaf63_cf4c_8601_d675,
    },
    Seed {
        name: "tagged",
        kind: Kind::FromPeer,
        fingerprint: 0x36c2_9809_ac03_c71d,
    },
    Seed {
        name: "tagged-bare",
        kind: Kind::FromPeer,
        fingerprint: 0x6ad2_6a20_123b_a583,
    },
    Seed {
        name: "tags",
        kind: Kind::ToPeer,
        fingerprint: 0xca91_05a7_086f_93b2,
    },
    Seed {
        name: "taken",
        kind: Kind::FromPeer,
        fingerprint: 0xaf63_b84c_8601_af60,
    },
    Seed {
        name: "unconsulted",
        kind: Kind::FromPeer,
        fingerprint: 0xaf63_cd4c_8601_d30f,
    },
    Seed {
        name: "viewing",
        kind: Kind::ToServer,
        fingerprint: 0x1817_61ca_ac82_a7e6,
    },
    Seed {
        name: "welcome",
        kind: Kind::FromServer,
        fingerprint: 0x297b_3fd1_3f79_ba3d,
    },
    Seed {
        name: "welcome-incarnated",
        kind: Kind::FromServer,
        fingerprint: 0xd71c_f0df_c6bf_8244,
    },
];

const SPACE_BYTES: usize = 16;

const TOKEN_BYTES: usize = 32;

const MBID: &str = "83d91898-7763-47d7-b03b-b92132375c47";

#[derive(Debug, PartialEq, Eq)]
enum Frame {
    ToServer(ToServer),
    FromServer(FromServer),
    ToPeer(ToPeer),
    FromPeer(FromPeer),
}

impl Frame {
    fn encoded(&self) -> Vec<u8> {
        match self {
            Self::ToServer(frame) => frame.encoded(),
            Self::FromServer(frame) => frame.encoded(),
            Self::ToPeer(frame) => frame.encoded(),
            Self::FromPeer(frame) => frame.encoded(),
        }
    }

    fn read_as_its_kind(&self, bytes: &[u8]) -> Result<Self, Error> {
        match self {
            Self::ToServer(_) => ToServer::decoded(bytes).map(Self::ToServer),
            Self::FromServer(_) => FromServer::decoded(bytes).map(Self::FromServer),
            Self::ToPeer(_) => ToPeer::decoded(bytes).map(Self::ToPeer),
            Self::FromPeer(_) => FromPeer::decoded(bytes).map(Self::FromPeer),
        }
    }
}

fn device(byte: u8) -> DeviceId {
    DeviceId([byte; DEVICE_BYTES])
}

fn space(byte: u8) -> SpaceId {
    SpaceId([byte; SPACE_BYTES])
}

fn token(byte: u8) -> Token {
    Token([byte; TOKEN_BYTES])
}

fn mbid() -> Mbid {
    Mbid::new(MBID).expect("an mbid")
}

fn song(title: &str) -> Recording {
    Recording {
        mbid: Some(mbid()),
        title: title.into(),
        artist: "Pink Floyd".into(),
        album: "Meddle".into(),
        length: Some(Duration::from_millis(1_403_000)),
    }
}

fn song_without_an_mbid() -> Recording {
    Recording {
        mbid: None,
        title: "Fearless".into(),
        artist: "Pink Floyd".into(),
        album: "Meddle".into(),
        length: None,
    }
}

fn stamp() -> Stamp {
    Stamp {
        nanos: -5,
        device: device(7),
    }
}

fn blob(first: u8, length: usize) -> Vec<u8> {
    (0..length)
        .map(|at| first.wrapping_add(u8::try_from(at).expect("a short blob")))
        .collect()
}

fn changes() -> Vec<Change> {
    vec![
        Change::Favourite(FavouriteNote {
            recording: song("Time"),
            favoured: true,
            stamp: stamp(),
        }),
        Change::Listen(Listen {
            device: device(3),
            id: 41,
            recording: song_without_an_mbid(),
            at: 1_700_000_000_000_000_000,
            heard: Duration::from_secs(215),
        }),
        Change::Device(DeviceNote {
            device: device(3),
            name: "Kitchen".into(),
            platform: Platform::Linux,
            stamp: stamp(),
        }),
    ]
}

fn tags() -> SongTags {
    SongTags {
        album_artist: Some("Pink Floyd".into()),
        date: Some("1971-10-30".into()),
        track_number: Some(6),
        track_total: Some(6),
        disc_number: Some(1),
        disc_total: Some(2),
        compilation: true,
    }
}

fn frames_a_released_build_wrote() -> Vec<(&'static str, Frame)> {
    let seen_at: SocketAddr = "203.0.113.9:40000".parse().expect("an address");
    let punched_at: SocketAddr = "[2001:db8::1]:443".parse().expect("an address");

    vec![
        (
            "welcome",
            Frame::FromServer(FromServer::Welcome {
                online: vec![device(4)],
                seen_at,
                head: 12,
                incarnation: None,
            }),
        ),
        (
            "append",
            Frame::ToServer(ToServer::Append {
                blobs: vec![blob(1, 40), blob(9, 28)],
            }),
        ),
        (
            "compact",
            Frame::ToServer(ToServer::Compact {
                through: 12,
                parts: vec![blob(5, 60)],
            }),
        ),
        (
            "punch",
            Frame::ToServer(ToServer::Punch {
                to: device(6),
                sealed: blob(2, 36),
            }),
        ),
        ("bye", Frame::ToServer(ToServer::Bye)),
        ("relaying", Frame::ToServer(ToServer::Relaying)),
        (
            "relay",
            Frame::ToServer(ToServer::Relay {
                to: device(8),
                datagram: blob(4, 33),
            }),
        ),
        (
            "share",
            Frame::ToServer(ToServer::Share {
                version: 1,
                space: space(1),
                token: token(2),
                viewer: [3; TOKEN_BYTES],
                device: device(4),
                after: 17,
            }),
        ),
        (
            "close",
            Frame::ToServer(ToServer::Close {
                sealed: blob(7, 30),
            }),
        ),
        (
            "room-say",
            Frame::ToServer(ToServer::RoomSay {
                to: Some(device(9)),
                sealed: blob(3, 32),
            }),
        ),
        (
            "room-say-all",
            Frame::ToServer(ToServer::RoomSay {
                to: None,
                sealed: blob(3, 32),
            }),
        ),
        (
            "room-kick",
            Frame::ToServer(ToServer::RoomKick { member: device(10) }),
        ),
        (
            "snapshot",
            Frame::FromServer(FromServer::Snapshot {
                through: 12,
                parts: vec![blob(1, 50), blob(60, 29)],
            }),
        ),
        (
            "records-tombstoned",
            Frame::FromServer(FromServer::Records {
                entries: vec![
                    Record {
                        seq: 13,
                        blob: Some(blob(11, 44)),
                    },
                    Record {
                        seq: 14,
                        blob: None,
                    },
                ],
            }),
        ),
        (
            "appended",
            Frame::FromServer(FromServer::Appended { count: 2 }),
        ),
        (
            "arrived",
            Frame::FromServer(FromServer::Arrived { device: device(2) }),
        ),
        (
            "left",
            Frame::FromServer(FromServer::Left { device: device(2) }),
        ),
        (
            "punched",
            Frame::FromServer(FromServer::Punched {
                device: device(3),
                address: punched_at,
                sealed: blob(8, 34),
            }),
        ),
        (
            "compacted",
            Frame::FromServer(FromServer::Compacted { through: 7 }),
        ),
        (
            "refused",
            Frame::FromServer(FromServer::Refused(Refusal::SpaceFull)),
        ),
        (
            "refused-stale",
            Frame::FromServer(FromServer::Refused(Refusal::Stale)),
        ),
        (
            "relayed",
            Frame::FromServer(FromServer::Relayed {
                from: device(5),
                datagram: blob(9, 40),
            }),
        ),
        (
            "closed",
            Frame::FromServer(FromServer::Closed {
                sealed: blob(6, 30),
            }),
        ),
        (
            "heads",
            Frame::FromServer(FromServer::Heads {
                heads: vec![
                    SpaceHead {
                        space: space(1),
                        head: 12,
                        closed: false,
                    },
                    SpaceHead {
                        space: space(2),
                        head: 0,
                        closed: true,
                    },
                ],
            }),
        ),
        (
            "room-welcome",
            Frame::FromServer(FromServer::RoomWelcome {
                members: vec![device(1), device(2)],
                host: device(1),
            }),
        ),
        (
            "room-heard",
            Frame::FromServer(FromServer::RoomHeard {
                from: device(2),
                sealed: blob(5, 32),
            }),
        ),
        (
            "room-arrived",
            Frame::FromServer(FromServer::RoomArrived { member: device(4) }),
        ),
        (
            "room-left",
            Frame::FromServer(FromServer::RoomLeft { member: device(4) }),
        ),
        ("room-ended", Frame::FromServer(FromServer::RoomEnded)),
        ("holdings", Frame::ToPeer(ToPeer::Holdings)),
        (
            "fetch",
            Frame::ToPeer(ToPeer::Fetch {
                recording: song("Echoes"),
                offset: 1 << 40,
                bound: Bound::Unsaid,
            }),
        ),
        (
            "meet",
            Frame::ToPeer(ToPeer::Meet {
                device: device(3),
                records: true,
            }),
        ),
        (
            "changes-whole",
            Frame::ToPeer(ToPeer::Changes {
                whole: true,
                changes: changes(),
            }),
        ),
        (
            "changes",
            Frame::ToPeer(ToPeer::Changes {
                whole: false,
                changes: changes(),
            }),
        ),
        (
            "tags",
            Frame::ToPeer(ToPeer::Tags {
                recording: song("Echoes"),
            }),
        ),
        (
            "covers",
            Frame::ToPeer(ToPeer::Covers {
                releases: vec![mbid()],
            }),
        ),
        (
            "kept",
            Frame::ToPeer(ToPeer::Kept {
                asker: device(2),
                after: 40,
                most: 128,
            }),
        ),
        (
            "held",
            Frame::FromPeer(FromPeer::Held {
                items: vec![
                    Held {
                        recording: song("One of These Days"),
                        size: 30_000_000,
                        extension: "flac".into(),
                    },
                    Held {
                        recording: song_without_an_mbid(),
                        size: 4_096,
                        extension: "mp3".into(),
                    },
                ],
            }),
        ),
        ("held-end", Frame::FromPeer(FromPeer::HeldEnd)),
        (
            "sending",
            Frame::FromPeer(FromPeer::Sending {
                size: 9000,
                extension: "flac".into(),
                revised: None,
            }),
        ),
        ("missing", Frame::FromPeer(FromPeer::Missing)),
        ("met", Frame::FromPeer(FromPeer::Met { device: device(4) })),
        ("taken", Frame::FromPeer(FromPeer::Taken)),
        ("tagged", Frame::FromPeer(FromPeer::Tagged { tags: tags() })),
        (
            "tagged-bare",
            Frame::FromPeer(FromPeer::Tagged {
                tags: SongTags::default(),
            }),
        ),
        (
            "covered",
            Frame::FromPeer(FromPeer::Covered {
                release: mbid(),
                picture: blob(1, 24),
            }),
        ),
        ("covers-end", Frame::FromPeer(FromPeer::CoversEnd)),
        ("lyricsfiles-end", Frame::FromPeer(FromPeer::LyricsfilesEnd)),
        ("prints-end", Frame::FromPeer(FromPeer::PrintsEnd)),
    ]
}

const RECORDS_SEALED_SHORTER_THAN_A_SEAL: Seed = Seed {
    name: "records",
    kind: Kind::FromServer,
    fingerprint: 0x5f06_8b5d_af6d_afe6,
};

const LENGTH_OF_ITS_SEALED_BLOB: usize = 3;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn seeds_folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/seeds/sync")
}

fn fingerprint_of(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    })
}

fn bytes_of(seed: &Seed) -> Vec<u8> {
    let bytes = fs::read(seeds_folder().join(seed.name)).expect("a seed");

    assert_eq!(
        fingerprint_of(&bytes),
        seed.fingerprint,
        "the seed {} was edited; a frame once sent is frozen, so write a new seed beside it",
        seed.name
    );
    bytes
}

fn written_back(kind: Kind, bytes: &[u8]) -> Result<Vec<u8>, Error> {
    match kind {
        Kind::ToServer => ToServer::decoded(bytes).map(|frame| frame.encoded()),
        Kind::FromServer => FromServer::decoded(bytes).map(|frame| frame.encoded()),
        Kind::ToPeer => ToPeer::decoded(bytes).map(|frame| frame.encoded()),
        Kind::FromPeer => FromPeer::decoded(bytes).map(|frame| frame.encoded()),
        Kind::Bundle => decoded_bundle(bytes).map(|changes| encoded_bundle(&changes)),
    }
}

#[test]
fn every_frame_an_earlier_build_wrote_is_read_as_its_kind_and_written_back_byte_for_byte() {
    for seed in SEEDS {
        let bytes = bytes_of(seed);

        let written = written_back(seed.kind, &bytes).unwrap_or_else(|error| {
            panic!(
                "the seed {} no longer decodes as {:?}: {error}",
                seed.name, seed.kind
            )
        });

        assert_eq!(
            written, bytes,
            "the seed {} reads as {:?} but is written back otherwise",
            seed.name, seed.kind
        );
    }
}

#[test]
fn every_frame_a_released_build_wrote_is_what_this_build_writes_and_reads_from_the_same_values() {
    let expected = frames_a_released_build_wrote();

    for (name, frame) in &expected {
        let seed = SEEDS
            .iter()
            .find(|seed| seed.name == *name)
            .unwrap_or_else(|| panic!("the frame {name} has no seed"));
        let bytes = bytes_of(seed);

        let read = frame.read_as_its_kind(&bytes).unwrap_or_else(|error| {
            panic!(
                "the seed {name} no longer decodes as {:?}: {error}",
                seed.kind
            )
        });

        assert_eq!(
            frame.encoded(),
            bytes,
            "this build writes the frame {name} otherwise than the release that wrote the seed"
        );
        assert_eq!(
            &read, frame,
            "this build reads the seed {name} as other than the values the release wrote"
        );
    }
}

#[test]
fn every_seed_in_the_folder_is_held_by_a_kind() {
    let held: BTreeSet<&str> = SEEDS
        .iter()
        .chain([&RECORDS_SEALED_SHORTER_THAN_A_SEAL])
        .map(|seed| seed.name)
        .collect();

    let found: BTreeSet<String> = fs::read_dir(seeds_folder())
        .expect("the seeds")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();

    let unheld: Vec<&String> = found
        .iter()
        .filter(|name| !held.contains(name.as_str()))
        .collect();

    assert!(unheld.is_empty(), "seeds no kind holds: {unheld:?}");
    assert_eq!(found.len(), held.len());
}

#[test]
fn records_whose_blob_is_shorter_than_a_seal_are_refused_as_no_build_since_the_first_sends_them() {
    let seed = &RECORDS_SEALED_SHORTER_THAN_A_SEAL;
    let bytes = bytes_of(seed);

    let read = written_back(seed.kind, &bytes);

    assert!(
        matches!(
            read,
            Err(Error::Malformed(Malformed::Short {
                length: LENGTH_OF_ITS_SEALED_BLOB
            }))
        ),
        "{read:?}"
    );
}
