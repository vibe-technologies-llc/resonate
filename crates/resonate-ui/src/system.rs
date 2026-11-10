#[cfg(not(any(windows, target_os = "macos")))]
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

#[cfg(target_os = "macos")]
macro_rules! sound_system {
    () => {
        "Core Audio"
    };
}

pub(crate) use sound_system;
