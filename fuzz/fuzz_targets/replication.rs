#![no_main]

use libfuzzer_sys::fuzz_target;

const SEQUENCE_THE_SEEDS_CARRY: u64 = 101;

const BZIP2_OPENS: &[u8] = b"BZh";

fuzz_target!(|bytes: &[u8]| {
    if bytes.starts_with(BZIP2_OPENS)
        && let Ok(packet) = resonate_musicbrainz::packet_read(bytes, SEQUENCE_THE_SEEDS_CARRY)
    {
        assert_eq!(packet.sequence, SEQUENCE_THE_SEEDS_CARRY);
    }
    resonate_musicbrainz::read_pending_changes(bytes);
});
