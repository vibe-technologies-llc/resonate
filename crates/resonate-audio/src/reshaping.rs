use crate::{StreamRequest, Unreshapable};

pub fn refusal_of(open: &StreamRequest, asked: &StreamRequest) -> Option<Unreshapable> {
    if asked.target != open.target {
        Some(Unreshapable::AnotherTarget)
    } else if asked.spec != open.spec || asked.latency != open.latency {
        Some(Unreshapable::AnotherFormat)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use resonate_core::{ChannelLayout, SampleFormat, SampleRate, StreamSpec};

    use super::*;
    use crate::{LatencyRequest, MediaRole, SinkId};

    fn request() -> StreamRequest {
        StreamRequest {
            target: Some(SinkId::new(7)),
            spec: StreamSpec::new(
                SampleRate::HZ_44100,
                ChannelLayout::Stereo,
                SampleFormat::S16,
            ),
            latency: LatencyRequest::Auto,
            role: MediaRole::Music,
            media_name: "a song".to_owned(),
            force_graph_rate: false,
            no_convert: true,
            realtime: true,
        }
    }

    #[test]
    fn a_new_source_or_priority_on_the_same_client_is_taken_in_place() {
        let open = request();
        let lowered = StreamRequest {
            realtime: false,
            media_name: "another song".to_owned(),
            no_convert: false,
            ..request()
        };

        assert_eq!(refusal_of(&open, &open), None);
        assert_eq!(refusal_of(&open, &lowered), None);
    }

    #[test]
    fn another_device_format_or_buffer_needs_a_client_of_its_own() {
        let open = request();
        let elsewhere = StreamRequest {
            target: None,
            ..request()
        };
        let faster = StreamRequest {
            spec: StreamSpec::new(
                SampleRate::HZ_48000,
                ChannelLayout::Stereo,
                SampleFormat::S16,
            ),
            ..request()
        };
        let deeper = StreamRequest {
            latency: LatencyRequest::Duration(Duration::from_millis(40)),
            ..request()
        };

        assert_eq!(
            refusal_of(&open, &elsewhere),
            Some(Unreshapable::AnotherTarget)
        );
        assert_eq!(
            refusal_of(&open, &faster),
            Some(Unreshapable::AnotherFormat)
        );
        assert_eq!(
            refusal_of(&open, &deeper),
            Some(Unreshapable::AnotherFormat)
        );
    }
}
