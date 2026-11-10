use resonate_audio::NodeName;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Microphone {
    pub name: NodeName,
    pub description: String,
    pub is_default: bool,
}
