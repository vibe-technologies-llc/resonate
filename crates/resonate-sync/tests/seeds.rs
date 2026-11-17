use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use resonate_sync::{
    Error, Malformed,
    wire::{FromPeer, FromServer, ToPeer, ToServer, decoded_bundle, encoded_bundle},
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
        name: "bundle",
        kind: Kind::Bundle,
        fingerprint: 0xecc9_9aad_31d0_ee9f,
    },
    Seed {
        name: "compact-staged",
        kind: Kind::ToServer,
        fingerprint: 0xadd6_9e7c_a905_d2ca,
    },
    Seed {
        name: "consult",
        kind: Kind::ToPeer,
        fingerprint: 0x0c3c_238a_6569_946e,
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
        name: "heads-asked",
        kind: Kind::ToServer,
        fingerprint: 0x5469_6afd_cbd4_1b5a,
    },
    Seed {
        name: "hello",
        kind: Kind::ToServer,
        fingerprint: 0x936d_fc7c_5ea8_dbee,
    },
    Seed {
        name: "kept-end",
        kind: Kind::FromPeer,
        fingerprint: 0x086f_9e07_b51f_436d,
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
        name: "records-sealed",
        kind: Kind::FromServer,
        fingerprint: 0x85be_18ff_eba5_63d7,
    },
    Seed {
        name: "room",
        kind: Kind::ToServer,
        fingerprint: 0x87a3_21d4_d38c_ab70,
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
        name: "unconsulted",
        kind: Kind::FromPeer,
        fingerprint: 0xaf63_cd4c_8601_d30f,
    },
    Seed {
        name: "viewing",
        kind: Kind::ToServer,
        fingerprint: 0x1817_61ca_ac82_a7e6,
    },
];

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
