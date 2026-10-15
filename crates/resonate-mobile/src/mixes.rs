use resonate_core::Accent;

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MixAccent {
    Mauve,
    Blue,
    Teal,
    Green,
    Amber,
    Peach,
    Red,
}

impl From<Accent> for MixAccent {
    fn from(accent: Accent) -> Self {
        match accent {
            Accent::Mauve => Self::Mauve,
            Accent::Blue => Self::Blue,
            Accent::Teal => Self::Teal,
            Accent::Green => Self::Green,
            Accent::Amber => Self::Amber,
            Accent::Peach => Self::Peach,
            Accent::Red => Self::Red,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MixAccents {
    pub from: MixAccent,
    pub to: MixAccent,
}

#[uniffi::export]
pub fn named_accents(name: String) -> MixAccents {
    let (from, to) = Accent::named_pair(&name);

    MixAccents {
        from: from.into(),
        to: to.into(),
    }
}
