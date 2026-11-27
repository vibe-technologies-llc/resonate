#[cfg(target_os = "linux")]
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

#[cfg(target_os = "linux")]
fn this_test() -> String {
    std::thread::current()
        .name()
        .expect("a test runs on a thread named for it")
        .to_owned()
}

#[cfg(target_os = "linux")]
fn ran_one_test(report: &str) -> bool {
    report.contains(" 1 passed;")
}

mod capture;
mod microphone;
mod reconnect;
