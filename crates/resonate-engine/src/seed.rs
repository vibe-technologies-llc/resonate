use std::time::{SystemTime, UNIX_EPOCH};

const GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

pub fn from_clock() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64);
    if nanos == 0 { GOLDEN } else { nanos }
}
