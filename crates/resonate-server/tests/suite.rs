macro_rules! passed_over {
    ($($why:tt)*) => {{
        let why = format!($($why)*);

        assert!(
            std::env::var(REQUIRED_BY).as_deref() != Ok("required"),
            "{why}, and {REQUIRED_BY} asks for it"
        );

        eprintln!("skipped: {why}");
    }};
}

mod compaction;
mod firefox;
mod frames;
mod hifi;
mod packaging;
mod renewal;
mod scripted;
mod shared;
#[path = "../../resonate-sync/tests/shelf/mod.rs"]
mod shelf;
mod sync;
mod together;
mod upkeep;
mod web;
