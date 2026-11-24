use resonate_core::ZoneId;

use crate::{
    PlaybackPreferences,
    harness::{CatalogFile, Harness},
};

const VOLUME_KEPT_WITHIN: f32 = 0.001;

fn first_launch_with_a_kitchen(file: &CatalogFile) -> (Harness, ZoneId) {
    let harness = Harness::launched_over(file);

    let kitchen = harness
        .player
        .zone_added("Kitchen", 0, String::new())
        .expect("a second zone")
        .id;

    (harness, kitchen)
}

fn showing(harness: &Harness, zone: ZoneId) {
    harness
        .player
        .show_zone(zone.get())
        .expect("the zone shown");
}

fn zone_figures(harness: &Harness) -> Vec<(u32, String, i32)> {
    harness
        .player
        .zones()
        .into_iter()
        .map(|zone| (zone.id, zone.name, (zone.volume * 100.0).round() as i32))
        .collect()
}

fn volume_settled_at(harness: &Harness, wanted: f32) {
    harness.settled(|snapshot| (snapshot.volume - wanted).abs() < VOLUME_KEPT_WITHIN);
}

#[test]
fn a_new_player_over_the_saved_catalog_opens_each_zone_on_the_queue_it_was_left_with() {
    let file = CatalogFile::new();

    let kitchen = {
        let (harness, kitchen) = first_launch_with_a_kitchen(&file);

        harness.resumed("abcd", 2, 500);
        showing(&harness, kitchen);
        harness
            .player
            .load(harness.ids("dc"), 1, false)
            .expect("a queue for the kitchen");
        harness.queued_as("dc");
        harness.player.shutdown().expect("shut down");
        kitchen
    };

    let harness = Harness::launched_over(&file);

    harness.queued_as("abcd");
    harness.settled(|snapshot| snapshot.position_ms >= 500);

    let first = harness.playing();
    let zones = zone_figures(&harness);

    showing(&harness, kitchen);
    harness.queued_as("dc");
    harness.settled(|snapshot| snapshot.zone == kitchen.get() && snapshot.current.is_some());

    let second = harness.playing();

    assert_eq!(first, ('c', 2, 500));
    assert_eq!(
        zones,
        vec![
            (1, "Zone 1".to_owned(), 100),
            (kitchen.get(), "Kitchen".to_owned(), 100)
        ]
    );
    assert_eq!((second.0, second.1), ('c', 1));
}

#[test]
fn a_shuffled_zone_reopens_in_its_order_with_the_rows_played_next_and_its_neighbour_unshuffled() {
    let file = CatalogFile::new();

    let (kitchen, saved) = {
        let (harness, kitchen) = first_launch_with_a_kitchen(&file);

        harness.resumed("abcd", 0, 0);
        harness.player.set_shuffle(true).expect("shuffle on");
        harness.settled(|snapshot| snapshot.shuffle);
        harness
            .player
            .play_next(harness.ids("c"))
            .expect("a row played next");
        harness.settled(|snapshot| snapshot.next_count == 1);

        let saved = harness.arrangement();

        showing(&harness, kitchen);
        harness
            .player
            .load(harness.ids("dcba"), 0, false)
            .expect("a queue for the kitchen");
        harness.queued_as("dcba");
        harness.player.shutdown().expect("shut down");
        (kitchen, saved)
    };

    let harness = Harness::launched_over(&file);

    harness.settled(|snapshot| snapshot.shuffle && snapshot.next_count == 1);

    let first = harness.arrangement();
    let first_snapshot = harness.player.snapshot().expect("a snapshot");

    showing(&harness, kitchen);
    harness.queued_as("dcba");

    let second = harness.arrangement();
    let second_snapshot = harness.player.snapshot().expect("a snapshot");

    assert!(saved.2);
    assert!(saved.3.is_some());
    assert_eq!(first, saved);
    assert_eq!(first_snapshot.next_count, 1);
    assert!(first_snapshot.next_first.is_some());
    assert_eq!(
        (second.1.as_str(), second.2, second.3),
        ("dcba", false, None)
    );
    assert!(!second_snapshot.shuffle);
    assert_eq!(
        (second_snapshot.next_first, second_snapshot.next_count),
        (None, 0)
    );
}

#[test]
fn each_zones_volume_is_the_one_it_was_left_at_in_a_new_player() {
    let file = CatalogFile::new();

    let kitchen = {
        let (harness, kitchen) = first_launch_with_a_kitchen(&file);

        harness.player.set_volume(0.5).expect("the first quieter");
        volume_settled_at(&harness, 0.5);
        showing(&harness, kitchen);
        harness
            .player
            .set_volume(0.25)
            .expect("the kitchen quieter");
        volume_settled_at(&harness, 0.25);
        harness.player.shutdown().expect("shut down");
        kitchen
    };

    let harness = Harness::launched_over(&file);

    volume_settled_at(&harness, 0.5);

    let first = zone_figures(&harness);

    showing(&harness, kitchen);
    volume_settled_at(&harness, 0.25);

    assert_eq!(
        first,
        vec![
            (1, "Zone 1".to_owned(), 50),
            (kitchen.get(), "Kitchen".to_owned(), 25)
        ]
    );
    assert_eq!(harness.player.shown_zone(), kitchen.get());
}

#[test]
fn a_new_player_opens_empty_zones_at_their_volumes_while_carrying_queues_over_is_off() {
    let file = CatalogFile::new();

    let kitchen = {
        let (harness, kitchen) = first_launch_with_a_kitchen(&file);

        harness.resumed("abcd", 1, 0);
        harness.player.set_volume(0.5).expect("the first quieter");
        volume_settled_at(&harness, 0.5);
        showing(&harness, kitchen);
        harness
            .player
            .load(harness.ids("dc"), 0, false)
            .expect("a queue for the kitchen");
        harness.queued_as("dc");
        harness
            .player
            .configure_playback(PlaybackPreferences {
                carries_the_queue_over: false,
                ..PlaybackPreferences::default()
            })
            .expect("carrying the queue over turned off");
        harness.player.shutdown().expect("shut down");
        kitchen
    };

    let harness = Harness::launched_over(&file);

    volume_settled_at(&harness, 0.5);

    let first = harness.player.snapshot().expect("a snapshot");
    let first_queue = harness.queued();

    showing(&harness, kitchen);

    let second_queue = harness.queued();
    let second = harness.player.snapshot().expect("a snapshot");

    assert!(!harness.player.catalog.carries_queues_over().expect("read"));
    assert_eq!((first_queue.as_str(), second_queue.as_str()), ("", ""));
    assert!(first.current.is_none() && second.current.is_none());
    assert_eq!(harness.player.zones().len(), 2);
}
