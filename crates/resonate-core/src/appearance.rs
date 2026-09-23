use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Theme {
    Resonate,
    Midnight,
    Graphite,
    Plum,
    RosePine,
    RosePineMoon,
    CatppuccinMocha,
    CatppuccinMacchiato,
    CatppuccinFrappe,
    Nord,
    GruvboxDark,
    TokyoNight,
}

impl Theme {
    pub const ALL: [Self; 12] = [
        Self::Resonate,
        Self::Midnight,
        Self::Graphite,
        Self::Plum,
        Self::RosePine,
        Self::RosePineMoon,
        Self::CatppuccinMocha,
        Self::CatppuccinMacchiato,
        Self::CatppuccinFrappe,
        Self::Nord,
        Self::GruvboxDark,
        Self::TokyoNight,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Resonate => "resonate",
            Self::Midnight => "midnight",
            Self::Graphite => "graphite",
            Self::Plum => "plum",
            Self::RosePine => "rose-pine",
            Self::RosePineMoon => "rose-pine-moon",
            Self::CatppuccinMocha => "catppuccin-mocha",
            Self::CatppuccinMacchiato => "catppuccin-macchiato",
            Self::CatppuccinFrappe => "catppuccin-frappe",
            Self::Nord => "nord",
            Self::GruvboxDark => "gruvbox-dark",
            Self::TokyoNight => "tokyo-night",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Resonate => "Resonate",
            Self::Midnight => "Midnight",
            Self::Graphite => "Graphite",
            Self::Plum => "Plum",
            Self::RosePine => "Rosé Pine",
            Self::RosePineMoon => "Rosé Pine Moon",
            Self::CatppuccinMocha => "Catppuccin Mocha",
            Self::CatppuccinMacchiato => "Catppuccin Macchiato",
            Self::CatppuccinFrappe => "Catppuccin Frappé",
            Self::Nord => "Nord",
            Self::GruvboxDark => "Gruvbox Dark",
            Self::TokyoNight => "Tokyo Night",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|theme| theme.as_str() == text)
    }
}

impl fmt::Display for Theme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Accent {
    Mauve,
    Blue,
    Teal,
    Green,
    Amber,
    Peach,
    Red,
}

impl Accent {
    pub const ALL: [Self; 7] = [
        Self::Mauve,
        Self::Blue,
        Self::Teal,
        Self::Green,
        Self::Amber,
        Self::Peach,
        Self::Red,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mauve => "mauve",
            Self::Blue => "blue",
            Self::Teal => "teal",
            Self::Green => "green",
            Self::Amber => "amber",
            Self::Peach => "peach",
            Self::Red => "red",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Mauve => "Mauve",
            Self::Blue => "Blue",
            Self::Teal => "Teal",
            Self::Green => "Green",
            Self::Amber => "Amber",
            Self::Peach => "Peach",
            Self::Red => "Red",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|accent| accent.as_str() == text)
    }
}

impl fmt::Display for Accent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextSize {
    Small,
    Medium,
    Large,
}

impl TextSize {
    pub const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];

    pub const MIDDLING: f32 = 13.5;

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
        }
    }

    pub const fn root(self) -> f32 {
        match self {
            Self::Small => 12.0,
            Self::Medium => Self::MIDDLING,
            Self::Large => 15.5,
        }
    }

    pub fn scale(self) -> f32 {
        self.root() / Self::MIDDLING
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|size| size.as_str() == text)
    }
}

impl fmt::Display for TextSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Appearance {
    pub theme: Theme,
    pub accent: Option<Accent>,
    pub text_size: TextSize,
}

impl Appearance {
    pub const DEFAULT: Self = Self {
        theme: Theme::Resonate,
        accent: None,
        text_size: TextSize::Medium,
    };
}

impl Default for Appearance {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_reads_back_from_the_name_it_is_written_under() {
        for theme in Theme::ALL {
            assert_eq!(Theme::parse(theme.as_str()), Some(theme));
        }
    }

    #[test]
    fn every_accent_reads_back_from_the_name_it_is_written_under() {
        for accent in Accent::ALL {
            assert_eq!(Accent::parse(accent.as_str()), Some(accent));
        }
    }

    #[test]
    fn every_text_size_reads_back_from_the_name_it_is_written_under() {
        for size in TextSize::ALL {
            assert_eq!(TextSize::parse(size.as_str()), Some(size));
        }
    }

    #[test]
    fn a_palette_of_its_own_is_what_an_appearance_naming_no_accent_asks_for() {
        assert_eq!(Appearance::DEFAULT.accent, None);
    }

    #[test]
    fn a_name_no_theme_accent_or_size_claims_reads_as_nothing() {
        assert_eq!(Theme::parse("solarized"), None);
        assert_eq!(Accent::parse("chartreuse"), None);
        assert_eq!(TextSize::parse("enormous"), None);
    }

    #[test]
    fn no_two_themes_accents_or_sizes_share_a_name() {
        for theme in Theme::ALL {
            let sharing = Theme::ALL
                .into_iter()
                .filter(|other| other.as_str() == theme.as_str())
                .count();
            assert_eq!(sharing, 1, "{theme} is written under a name another claims");
        }
        for accent in Accent::ALL {
            let sharing = Accent::ALL
                .into_iter()
                .filter(|other| other.as_str() == accent.as_str())
                .count();
            assert_eq!(
                sharing, 1,
                "{accent} is written under a name another claims"
            );
        }
        for size in TextSize::ALL {
            let sharing = TextSize::ALL
                .into_iter()
                .filter(|other| other.as_str() == size.as_str())
                .count();
            assert_eq!(sharing, 1, "{size} is written under a name another claims");
        }
    }

    #[test]
    fn the_middling_size_is_the_one_every_other_is_measured_against() {
        assert_eq!(TextSize::Medium.root(), TextSize::MIDDLING);
        assert_eq!(TextSize::Medium.scale(), 1.0);
        assert!(TextSize::Small.scale() < 1.0);
        assert!(TextSize::Large.scale() > 1.0);
    }
}
