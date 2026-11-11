#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|datagram: &[u8]| {
    resonate_sync::read_a_stun_answer(datagram);
});
