#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum OutputEncoding {
    Pcm16,
    Pcm32,
    Float,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct OutputFormat {
    pub rate: u32,
    pub encoding: OutputEncoding,
    pub channels: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum OutputPath {
    Mixed,
    Exclusive,
    BitPerfect,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct OutputStream {
    pub format: OutputFormat,
    pub path: OutputPath,
}

#[uniffi::export(callback_interface)]
pub trait OutputMixer: Send + Sync {
    fn prefer_bit_perfect(&self, device: i32, format: OutputFormat) -> bool;
    fn mix_freely(&self, device: i32);
}
