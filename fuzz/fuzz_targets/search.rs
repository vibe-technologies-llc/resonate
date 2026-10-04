#![no_main]

use libfuzzer_sys::fuzz_target;
use resonate_library::{Column, Search};

fuzz_target!(|text: &str| {
    let search = Search::read(text);
    let written = search.to_string();
    let _ = Search::read(&written).to_string();
    let _ = search.reads();
    let _ = search.is_empty();
    for column in [Column::Title, Column::Artist, Column::Album] {
        let _ = search.lit(text, column);
    }
});
