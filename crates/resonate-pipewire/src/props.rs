use libspa::{
    pod::{Pod, Value, ValueArray, deserialize::PodDeserializer},
    sys,
};

use crate::StreamVolume;

const UNITY_WITHIN: f32 = 1e-6;

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StreamLevels {
    volume: Option<f32>,
    channel_volumes: Vec<f32>,
    muted: bool,
    heard: StreamVolume,
}

impl StreamLevels {
    pub(crate) fn absorb(&mut self, param: &Pod) -> Option<StreamVolume> {
        let (_, value) = PodDeserializer::deserialize_any_from(param.as_bytes()).ok()?;
        self.absorbed(&value)
    }

    fn absorbed(&mut self, value: &Value) -> Option<StreamVolume> {
        let Value::Object(object) = value else {
            return None;
        };
        for property in &object.properties {
            match (property.key, &property.value) {
                (sys::SPA_PROP_volume, Value::Float(volume)) => self.volume = Some(*volume),
                (sys::SPA_PROP_channelVolumes, Value::ValueArray(ValueArray::Float(volumes))) => {
                    self.channel_volumes.clone_from(volumes);
                }
                (sys::SPA_PROP_mute, Value::Bool(muted)) => self.muted = *muted,
                _ => {}
            }
        }

        let heard = self.heard();
        (heard != self.heard).then(|| {
            self.heard = heard;
            heard
        })
    }

    fn heard(&self) -> StreamVolume {
        let at_unity = |volume: &f32| (volume - 1.0).abs() <= UNITY_WITHIN;
        if self.muted {
            StreamVolume::Muted
        } else if self
            .volume
            .iter()
            .chain(&self.channel_volumes)
            .all(at_unity)
        {
            StreamVolume::Unity
        } else {
            StreamVolume::Scaled
        }
    }
}

#[cfg(test)]
mod tests {
    use libspa::pod::{Object, Property};

    use super::*;

    fn props(properties: Vec<Property>) -> Value {
        Value::Object(Object {
            type_: sys::SPA_TYPE_OBJECT_Props,
            id: sys::SPA_PARAM_Props,
            properties,
        })
    }

    fn channels(volumes: &[f32]) -> Property {
        Property::new(
            sys::SPA_PROP_channelVolumes,
            Value::ValueArray(ValueArray::Float(volumes.to_vec())),
        )
    }

    #[test]
    fn a_stream_restored_below_unity_is_heard_scaled_and_back_at_unity_is_heard_whole() {
        let mut levels = StreamLevels::default();

        assert_eq!(
            levels.absorbed(&props(vec![channels(&[0.4, 0.4])])),
            Some(StreamVolume::Scaled)
        );
        assert_eq!(levels.absorbed(&props(vec![channels(&[0.4, 0.4])])), None);
        assert_eq!(
            levels.absorbed(&props(vec![channels(&[1.0, 1.0])])),
            Some(StreamVolume::Unity)
        );
    }

    #[test]
    fn a_mute_told_alone_keeps_the_volumes_told_before_it() {
        let mut levels = StreamLevels::default();
        let muted = |on| props(vec![Property::new(sys::SPA_PROP_mute, Value::Bool(on))]);

        assert_eq!(levels.absorbed(&muted(true)), Some(StreamVolume::Muted));
        assert_eq!(levels.absorbed(&props(vec![channels(&[0.5, 1.0])])), None);
        assert_eq!(levels.absorbed(&muted(false)), Some(StreamVolume::Scaled));
        assert_eq!(
            levels.absorbed(&props(vec![Property::new(
                sys::SPA_PROP_volume,
                Value::Float(1.0)
            )])),
            None
        );
    }

    #[test]
    fn props_naming_no_level_leave_the_stream_at_unity() {
        let mut levels = StreamLevels::default();

        assert_eq!(levels.absorbed(&props(Vec::new())), None);
        assert_eq!(levels.heard(), StreamVolume::Unity);
    }
}
