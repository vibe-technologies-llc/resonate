use resonate_audio::NodeName;

use crate::Listening;
pub(crate) use crate::microphone::Microphone;

const CAPTURE_PREFIX: &str = "wasapi-capture:";
const SHARED_PREFIX: &str = "wasapi:";
const EXCLUSIVE_PREFIX: &str = "wasapi-exclusive:";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Endpoint {
    pub(crate) id: String,
    pub(crate) friendly_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Tap {
    Desktop,
    Microphone(Option<String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Output {
    Default,
    Endpoint { name: NodeName, id: String },
}

pub(crate) fn capture_node_of(endpoint: &str) -> NodeName {
    NodeName::new(format!("{CAPTURE_PREFIX}{endpoint}"))
}

pub(crate) fn capture_endpoint_of(node: &NodeName) -> Option<&str> {
    node.as_str().strip_prefix(CAPTURE_PREFIX)
}

pub(crate) fn render_endpoint_of(node: &NodeName) -> Option<&str> {
    let name = node.as_str();
    name.strip_prefix(EXCLUSIVE_PREFIX)
        .or_else(|| name.strip_prefix(SHARED_PREFIX))
}

pub(crate) fn microphones_of(
    endpoints: impl IntoIterator<Item = Endpoint>,
    default: Option<&str>,
) -> Vec<Microphone> {
    endpoints
        .into_iter()
        .map(|endpoint| Microphone {
            name: capture_node_of(&endpoint.id),
            is_default: default == Some(endpoint.id.as_str()),
            description: endpoint.friendly_name.unwrap_or(endpoint.id),
        })
        .collect()
}

pub(crate) fn tap_of(from: &Listening) -> Result<Tap, &NodeName> {
    match from {
        Listening::Desktop => Ok(Tap::Desktop),
        Listening::Microphone(None) => Ok(Tap::Microphone(None)),
        Listening::Microphone(Some(node)) => capture_endpoint_of(node)
            .map(|id| Tap::Microphone(Some(id.to_owned())))
            .ok_or(node),
    }
}

pub(crate) fn output_of(through: Option<&NodeName>) -> Result<Output, &NodeName> {
    match through {
        None => Ok(Output::Default),
        Some(name) => render_endpoint_of(name)
            .map(|id| Output::Endpoint {
                name: name.clone(),
                id: id.to_owned(),
            })
            .ok_or(name),
    }
}

#[cfg(test)]
mod tests {
    use resonate_wasapi::{Way, node_of};

    use super::*;

    const ENDPOINT: &str = "{0.0.1.00000000}.{8a2f7c1e-0000-4d3b-9a10-5c4e7b6f2d11}";

    #[test]
    fn a_capture_node_names_its_endpoint_and_nothing_else_does() {
        let node = capture_node_of(ENDPOINT);

        assert_eq!(capture_endpoint_of(&node), Some(ENDPOINT));
        assert_eq!(capture_endpoint_of(&node_of(Way::Shared, ENDPOINT)), None);
        assert_eq!(
            capture_endpoint_of(&NodeName::new("alsa_input.usb-mono".to_owned())),
            None
        );
    }

    #[test]
    fn a_node_the_backend_named_for_playback_names_its_endpoint_either_way() {
        for way in [Way::Shared, Way::Exclusive] {
            assert_eq!(
                render_endpoint_of(&node_of(way, ENDPOINT)),
                Some(ENDPOINT),
                "{way:?}"
            );
        }
        assert_eq!(render_endpoint_of(&capture_node_of(ENDPOINT)), None);
        assert_eq!(
            render_endpoint_of(&NodeName::new("alsa_output.pci".to_owned())),
            None
        );
    }

    #[test]
    fn microphones_keep_the_systems_order_name_themselves_and_mark_the_default() {
        let endpoints = [
            Endpoint {
                id: "one".to_owned(),
                friendly_name: Some("Headset (USB Audio)".to_owned()),
            },
            Endpoint {
                id: "two".to_owned(),
                friendly_name: None,
            },
        ];

        let found = microphones_of(endpoints, Some("two"));

        assert_eq!(
            found,
            vec![
                Microphone {
                    name: capture_node_of("one"),
                    description: "Headset (USB Audio)".to_owned(),
                    is_default: false,
                },
                Microphone {
                    name: capture_node_of("two"),
                    description: "two".to_owned(),
                    is_default: true,
                },
            ]
        );
    }

    #[test]
    fn no_default_marks_no_microphone() {
        let found = microphones_of(
            [Endpoint {
                id: "one".to_owned(),
                friendly_name: None,
            }],
            None,
        );

        assert!(found.iter().all(|microphone| !microphone.is_default));
    }

    #[test]
    fn a_listening_chosen_by_a_windows_name_is_read_back() {
        let chosen = Listening::Microphone(Some(capture_node_of(ENDPOINT)));

        assert_eq!(Listening::named(&chosen.written()), chosen);
    }

    #[test]
    fn where_to_listen_becomes_a_tap_and_a_name_from_another_system_is_refused() {
        let chosen = capture_node_of(ENDPOINT);
        let foreign = NodeName::new("alsa_input.usb-mono".to_owned());

        assert_eq!(tap_of(&Listening::Desktop), Ok(Tap::Desktop));
        assert_eq!(
            tap_of(&Listening::Microphone(None)),
            Ok(Tap::Microphone(None))
        );
        assert_eq!(
            tap_of(&Listening::Microphone(Some(chosen))),
            Ok(Tap::Microphone(Some(ENDPOINT.to_owned())))
        );
        assert_eq!(
            tap_of(&Listening::Microphone(Some(foreign.clone()))),
            Err(&foreign)
        );
    }

    #[test]
    fn where_to_play_is_the_default_or_a_named_endpoint_and_nothing_else() {
        let named = node_of(Way::Exclusive, ENDPOINT);
        let foreign = NodeName::new("alsa_output.pci".to_owned());

        assert_eq!(output_of(None), Ok(Output::Default));
        assert_eq!(
            output_of(Some(&named)),
            Ok(Output::Endpoint {
                name: named.clone(),
                id: ENDPOINT.to_owned(),
            })
        );
        assert_eq!(output_of(Some(&foreign)), Err(&foreign));
    }
}
