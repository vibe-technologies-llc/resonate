use resonate_audio::NodeName;

use crate::Listening;

const CAPTURE_PREFIX: &str = "coreaudio-capture:";
const SHARED_PREFIX: &str = "coreaudio:";
const HOG_PREFIX: &str = "coreaudio-hog:";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Source<'a> {
    TheDesktop,
    Microphone(Option<&'a str>),
}

pub(crate) fn capture_node_of(uid: &str) -> NodeName {
    NodeName::new(format!("{CAPTURE_PREFIX}{uid}"))
}

fn capture_uid_of(node: &NodeName) -> Option<&str> {
    node.as_str().strip_prefix(CAPTURE_PREFIX)
}

fn render_uid_of(node: &NodeName) -> Option<&str> {
    let name = node.as_str();
    name.strip_prefix(HOG_PREFIX)
        .or_else(|| name.strip_prefix(SHARED_PREFIX))
}

pub(crate) fn source_of(from: &Listening) -> Result<Source<'_>, &NodeName> {
    match from {
        Listening::Desktop => Ok(Source::TheDesktop),
        Listening::Microphone(None) => Ok(Source::Microphone(None)),
        Listening::Microphone(Some(node)) => capture_uid_of(node)
            .map(|uid| Source::Microphone(Some(uid)))
            .ok_or(node),
    }
}

pub(crate) fn output_of(through: Option<&NodeName>) -> Result<Option<&str>, &NodeName> {
    match through {
        None => Ok(None),
        Some(name) => render_uid_of(name).map(Some).ok_or(name),
    }
}

#[cfg(test)]
mod tests {
    use resonate_coreaudio::{Way, node_of};

    use super::*;

    const UID: &str = "AppleUSBAudioEngine:Maker:Interface:1";

    #[test]
    fn a_capture_node_names_its_device_and_nothing_else_does() {
        let node = capture_node_of(UID);

        assert_eq!(capture_uid_of(&node), Some(UID));
        assert_eq!(capture_uid_of(&node_of(Way::Shared, UID)), None);
    }

    #[test]
    fn the_desktop_is_tapped_and_a_microphone_is_named_by_its_capture_node() {
        let named = Listening::Microphone(Some(capture_node_of(UID)));
        let stranger = NodeName::new("alsa_input.usb");

        assert_eq!(source_of(&Listening::Desktop), Ok(Source::TheDesktop));
        assert_eq!(
            source_of(&Listening::Microphone(None)),
            Ok(Source::Microphone(None))
        );
        assert_eq!(source_of(&named), Ok(Source::Microphone(Some(UID))));
        assert_eq!(
            source_of(&Listening::Microphone(Some(stranger.clone()))),
            Err(&stranger)
        );
    }

    #[test]
    fn a_sweep_plays_through_the_device_either_of_its_sinks_names() {
        let shared = node_of(Way::Shared, UID);
        let hogged = node_of(Way::Hog, UID);
        let capture = capture_node_of(UID);

        assert_eq!(output_of(None), Ok(None));
        assert_eq!(output_of(Some(&shared)), Ok(Some(UID)));
        assert_eq!(output_of(Some(&hogged)), Ok(Some(UID)));
        assert_eq!(output_of(Some(&capture)), Err(&capture));
    }
}
