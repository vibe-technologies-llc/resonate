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

#[cfg(windows)]
fn named<Test>(test: &Test) -> String {
    let (_, within_the_suite) = std::any::type_name_of_val(test)
        .split_once("::")
        .expect("a module of the suite");

    within_the_suite.to_owned()
}

mod bundle;
mod cli;
mod console;
mod dragging;
mod icons;
mod installer;
mod packaging;
mod signals;
mod signing;
mod terminal;
