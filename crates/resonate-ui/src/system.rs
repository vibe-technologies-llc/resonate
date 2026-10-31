#[cfg(not(windows))]
macro_rules! sound_system {
    () => {
        "PipeWire"
    };
}

#[cfg(windows)]
macro_rules! sound_system {
    () => {
        "Windows"
    };
}

pub(crate) use sound_system;
