use resonate_words::{
    Area, Arg, Brevity, Conventions, Language, day_and_month, day_month_and_year, decimal, grouped,
    month_name, settle, settled_conventions, weekday_name,
};

static SONGS: Area = Area::new(
    &["songs"],
    &[(
        Language::English,
        "songs = { $count ->\n    [one] { $count } song\n   *[other] { $count } songs\n}\n",
    )],
);

#[test]
fn every_figure_and_date_follows_the_region_settled_and_the_catalog_keeps_its_plurals() {
    let first = settled_conventions();
    assert_eq!(first, Conventions::default());
    assert_eq!(grouped(1_234_567), "1,234,567");
    assert_eq!(day_month_and_year(2026, 11, 13), "Nov 13, 2026");

    settle(&Conventions::from_tags(&["en-GB"]));
    assert_eq!(day_month_and_year(2026, 11, 13), "13 Nov 2026");
    assert_eq!(day_and_month(Brevity::Full, 11, 13), "13 November");
    assert_eq!(month_name(Brevity::Full, 11), "November");
    assert_eq!(weekday_name(Brevity::Brief, 4), "Fri");

    settle(&Conventions::from_tags(&["de-DE"]));
    assert_eq!(grouped(1_234_567), "1.234.567");
    assert_eq!(decimal(1234.5, 1), "1.234,5");
    assert_eq!(
        SONGS.formatted(0, [("count", Arg::from(12_345))]),
        "12.345 songs"
    );
    assert_eq!(SONGS.formatted(0, [("count", Arg::from(1))]), "1 song");

    settle(&Conventions::from_tags(&["hi-IN"]));
    assert_eq!(
        SONGS.formatted(0, [("count", Arg::from(1_234_567))]),
        "12,34,567 songs"
    );

    settle(&first);
    assert_eq!(grouped(1_234_567), "1,234,567");
}
