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

fn this_test() -> String {
    std::thread::current()
        .name()
        .expect("a test runs on a thread named for it")
        .to_owned()
}

mod cues;
mod deletions;
mod discography;
mod finding;
mod grouping;
mod history;
mod identify;
mod keeps;
mod landing;
mod links;
mod lists;
mod lossless;
mod organise;
mod passes;
mod playlists;
mod polls;
mod retag;
mod scanned;
mod scanning;
mod search;
mod sheets;
mod stored;
mod studies;
mod suggestions;
mod support;
mod synced;
mod undo;
mod vault;
mod wants;
mod watching;
