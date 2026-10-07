use resonate_core::AUDIO_EXTENSIONS;

const EXTENSION_SLOTS: usize = u32::BITS as usize;
const _: () = assert!(!AUDIO_EXTENSIONS.is_empty() && AUDIO_EXTENSIONS.len() <= EXTENSION_SLOTS);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MusicExtensions(u32);

impl Default for MusicExtensions {
    fn default() -> Self {
        Self(u32::MAX >> (EXTENSION_SLOTS - AUDIO_EXTENSIONS.len()))
    }
}

impl MusicExtensions {
    pub fn parse(extensions: &[&str]) -> Option<Self> {
        let mut selected = Self(0);
        for extension in extensions {
            let extension = extension.trim().trim_start_matches('.');
            let index = AUDIO_EXTENSIONS
                .iter()
                .position(|held| held.eq_ignore_ascii_case(extension))?;
            selected.0 |= 1 << index;
        }
        Some(selected)
    }

    pub fn selected(self) -> impl Iterator<Item = &'static str> {
        AUDIO_EXTENSIONS
            .iter()
            .enumerate()
            .filter_map(move |(index, extension)| {
                (self.0 & (1 << index) != 0).then_some(*extension)
            })
    }

    pub fn toggled(self, extension: &str) -> Self {
        match AUDIO_EXTENSIONS.iter().position(|held| *held == extension) {
            Some(index) => Self(self.0 ^ (1 << index)),
            None => self,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MinimumLength(u16);

impl MinimumLength {
    pub const MAX_SECONDS: u16 = 10 * 60;

    pub const fn new(seconds: u16) -> Option<Self> {
        if seconds <= Self::MAX_SECONDS {
            Some(Self(seconds))
        } else {
            None
        }
    }

    pub const fn seconds(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MusicFilters {
    pub extensions: MusicExtensions,
    pub minimum_length: MinimumLength,
}

impl MusicFilters {
    pub(crate) fn predicate(self, track: &str) -> Option<String> {
        let mut conditions = Vec::new();
        if self.extensions != MusicExtensions::default() {
            let extensions: Vec<String> = self
                .extensions
                .selected()
                .map(|extension| {
                    format!(
                        "lower(substr({track}.path, -{})) = '.{extension}'",
                        extension.len() + 1
                    )
                })
                .collect();
            conditions.push(if extensions.is_empty() {
                "0".to_owned()
            } else {
                format!("({})", extensions.join(" OR "))
            });
        }
        if self.minimum_length.seconds() > 0 {
            conditions.push(format!(
                "{track}.duration >= {} * {track}.sample_rate",
                self.minimum_length.seconds()
            ));
        }
        (!conditions.is_empty()).then(|| conditions.join(" AND "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_choices_are_supported_case_insensitive_and_accept_a_dot() {
        let extensions =
            MusicExtensions::parse(&[".MP3", "flac", ".mp3"]).expect("supported extensions");
        assert_eq!(extensions.selected().collect::<Vec<_>>(), ["flac", "mp3"]);
        assert!(MusicExtensions::parse(&[".exe"]).is_none());
        assert_eq!(
            MusicExtensions::parse(&[])
                .expect("no extensions")
                .selected()
                .count(),
            0
        );
        assert_eq!(
            MusicExtensions::default().selected().count(),
            AUDIO_EXTENSIONS.len()
        );
    }

    #[test]
    fn the_minimum_length_is_bounded_and_defaults_leave_the_query_alone() {
        assert_eq!(MinimumLength::new(600).expect("ten minutes").seconds(), 600);
        assert!(MinimumLength::new(601).is_none());
        assert!(MusicFilters::default().predicate("tracks").is_none());
    }
}
