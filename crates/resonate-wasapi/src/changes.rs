use resonate_audio::{SinkChange, SinkInfo};

pub fn changes_between(before: &[SinkInfo], after: &[SinkInfo]) -> Vec<SinkChange> {
    let mut changes = Vec::new();
    for gone in before
        .iter()
        .filter(|held| after.iter().all(|now| now.id != held.id))
    {
        changes.push(SinkChange::Removed(gone.id));
    }
    for now in after {
        match before.iter().find(|held| held.id == now.id) {
            None => changes.push(SinkChange::Added(now.id)),
            Some(held) if held.formats != now.formats || held.current_rate != now.current_rate => {
                changes.push(SinkChange::Reformatted(now.id));
            }
            Some(_) => {}
        }
    }
    let default_of = |sinks: &[SinkInfo]| sinks.iter().find(|sink| sink.is_default).map(|s| s.id);
    if default_of(before) != default_of(after) {
        changes.push(SinkChange::DefaultChanged);
    }
    changes
}

#[cfg(test)]
mod tests {
    use resonate_audio::{NodeName, SinkId};

    use super::*;

    fn sink(id: u32, is_default: bool) -> SinkInfo {
        SinkInfo {
            id: SinkId::new(id),
            name: NodeName::new(format!("wasapi:{id}")),
            description: String::new(),
            is_default,
            is_hardware: true,
            port: None,
            profile: None,
            formats: Vec::new(),
            allowed_rates: Vec::new(),
            current_rate: None,
        }
    }

    #[test]
    fn a_device_plugged_in_and_one_taken_out_are_told() {
        let before = [sink(1, true), sink(2, false)];
        let after = [sink(1, true), sink(3, false)];

        let changes = changes_between(&before, &after);

        assert!(changes.contains(&SinkChange::Removed(SinkId::new(2))));
        assert!(changes.contains(&SinkChange::Added(SinkId::new(3))));
        assert!(!changes.contains(&SinkChange::DefaultChanged));
    }

    #[test]
    fn a_default_that_moved_is_told_once() {
        let before = [sink(1, true), sink(2, false)];
        let after = [sink(1, false), sink(2, true)];

        assert_eq!(
            changes_between(&before, &after),
            [SinkChange::DefaultChanged]
        );
    }

    #[test]
    fn a_survey_that_found_the_same_devices_tells_nothing() {
        let sinks = [sink(1, true), sink(2, false)];

        assert!(changes_between(&sinks, &sinks).is_empty());
    }

    #[test]
    fn a_rate_the_mix_moved_to_is_a_reformat() {
        use resonate_core::SampleRate;

        let before = [sink(1, true)];
        let mut moved = sink(1, true);
        moved.current_rate = Some(SampleRate::HZ_48000);

        assert_eq!(
            changes_between(&before, &[moved]),
            [SinkChange::Reformatted(SinkId::new(1))]
        );
    }
}
