#![no_main]

use libfuzzer_sys::fuzz_target;
use resonate_sync::{
    party::PartySaid,
    shared::{SharedList, decoded_ops, decoded_seats, encoded_ops, encoded_seats, invitation_in},
};

fuzz_target!(|bytes: &[u8]| {
    if let Ok(said) = PartySaid::decoded(bytes) {
        assert_eq!(PartySaid::decoded(&said.encoded()).ok(), Some(said));
    }
    if let Ok(seats) = decoded_seats(bytes) {
        assert_eq!(decoded_seats(&encoded_seats(&seats)).ok(), Some(seats));
    }
    if let Ok(ops) = decoded_ops(bytes) {
        assert_eq!(decoded_ops(&encoded_ops(&ops)).ok(), Some(ops.clone()));

        let mut forwards = SharedList::default();
        let mut backwards = SharedList::default();
        for op in &ops {
            forwards.apply(op);
        }
        for op in ops.iter().rev() {
            backwards.apply(op);
        }

        let entries = |list: &SharedList| {
            let mut held: Vec<_> = list.rows().iter().map(|row| row.entry).collect();
            held.sort_unstable();
            held
        };

        assert_eq!(entries(&forwards), entries(&backwards));
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        let _ = invitation_in(text);
    }
});
