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

#[cfg(all(unix, not(target_os = "macos")))]
fn named<Test>(test: &Test) -> String {
    let (_, within_the_suite) = std::any::type_name_of_val(test)
        .split_once("::")
        .expect("a module of the suite");

    within_the_suite.to_owned()
}

fn ran_one_test(report: &str) -> bool {
    report.contains(" 1 passed;")
}

#[path = "../src/blocking.rs"]
#[allow(dead_code)]
mod blocking;
mod bus;
mod name;
mod trash;
