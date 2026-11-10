use resonate_audio::NodeName;

const SHARED_PREFIX: &str = "coreaudio:";
const HOG_PREFIX: &str = "coreaudio-hog:";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Way {
    Shared,
    Hog,
}

impl Way {
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Shared => SHARED_PREFIX,
            Self::Hog => HOG_PREFIX,
        }
    }
}

pub fn node_of(way: Way, uid: &str) -> NodeName {
    NodeName::new(format!("{}{uid}", way.prefix()))
}

pub fn device_of(node: &NodeName) -> Option<(Way, &str)> {
    let name = node.as_str();
    name.strip_prefix(HOG_PREFIX)
        .map(|uid| (Way::Hog, uid))
        .or_else(|| {
            name.strip_prefix(SHARED_PREFIX)
                .map(|uid| (Way::Shared, uid))
        })
}

#[cfg(test)]
mod tests {
    use resonate_audio::sink_id_of;

    use super::*;

    #[test]
    fn a_node_names_its_device_and_the_way_it_is_opened() {
        let shared = node_of(Way::Shared, "BuiltInSpeakerDevice");
        let hogged = node_of(Way::Hog, "AppleUSBAudioEngine:DAC:1");

        assert_eq!(
            device_of(&shared),
            Some((Way::Shared, "BuiltInSpeakerDevice"))
        );
        assert_eq!(
            device_of(&hogged),
            Some((Way::Hog, "AppleUSBAudioEngine:DAC:1"))
        );
        assert_eq!(device_of(&NodeName::new("wasapi:endpoint")), None);
    }

    #[test]
    fn the_same_device_opened_two_ways_is_two_sinks() {
        let shared = node_of(Way::Shared, "device");
        let hogged = node_of(Way::Hog, "device");

        assert_ne!(sink_id_of(&shared), sink_id_of(&hogged));
        assert_eq!(
            sink_id_of(&shared),
            sink_id_of(&node_of(Way::Shared, "device"))
        );
    }
}
