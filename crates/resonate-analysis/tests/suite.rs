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

mod analysis;
mod references;
mod turns;
