use resonate_audio::NodeName;

const SHARED_PREFIX: &str = "wasapi:";
const EXCLUSIVE_PREFIX: &str = "wasapi-exclusive:";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Way {
    Shared,
    Exclusive,
}

impl Way {
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::Shared => SHARED_PREFIX,
            Self::Exclusive => EXCLUSIVE_PREFIX,
        }
    }

    pub const fn described(self) -> &'static str {
        match self {
            Self::Shared => "",
            Self::Exclusive => " (WASAPI exclusive)",
        }
    }
}

pub fn node_of(way: Way, endpoint: &str) -> NodeName {
    NodeName::new(format!("{}{endpoint}", way.prefix()))
}

pub fn endpoint_of(node: &NodeName) -> Option<(Way, &str)> {
    let name = node.as_str();
    name.strip_prefix(EXCLUSIVE_PREFIX)
        .map(|endpoint| (Way::Exclusive, endpoint))
        .or_else(|| {
            name.strip_prefix(SHARED_PREFIX)
                .map(|endpoint| (Way::Shared, endpoint))
        })
}

#[cfg(test)]
mod tests {
    use resonate_audio::sink_id_of;

    use super::*;

    #[test]
    fn a_node_names_its_endpoint_and_the_way_it_is_opened() {
        let shared = node_of(Way::Shared, "{0.0.0.00000000}.{abc}");
        let exclusive = node_of(Way::Exclusive, "{0.0.0.00000000}.{abc}");

        assert_eq!(
            endpoint_of(&shared),
            Some((Way::Shared, "{0.0.0.00000000}.{abc}"))
        );
        assert_eq!(
            endpoint_of(&exclusive),
            Some((Way::Exclusive, "{0.0.0.00000000}.{abc}"))
        );
        assert_eq!(endpoint_of(&NodeName::new("alsa_output.pci")), None);
    }

    #[test]
    fn the_same_endpoint_opened_two_ways_is_two_sinks() {
        let shared = node_of(Way::Shared, "endpoint");
        let exclusive = node_of(Way::Exclusive, "endpoint");

        assert_ne!(sink_id_of(&shared), sink_id_of(&exclusive));
        assert_eq!(
            sink_id_of(&shared),
            sink_id_of(&node_of(Way::Shared, "endpoint"))
        );
    }
}
