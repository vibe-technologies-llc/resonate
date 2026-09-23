use std::fmt;

const SHORTEST_SNOWFLAKE: usize = 17;
const LONGEST_SNOWFLAKE: usize = 20;
const LONGEST_ICON: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AppId(u64);

impl AppId {
    pub fn parse(text: &str) -> Option<Self> {
        let digits = text.trim();
        let sized = (SHORTEST_SNOWFLAKE..=LONGEST_SNOWFLAKE).contains(&digits.len());
        if !sized || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        digits.parse().ok().map(Self)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for AppId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Icon(Box<str>);

impl Icon {
    pub fn parse(text: &str) -> Option<Self> {
        let named = text.trim();
        let plain = !named.is_empty()
            && named.len() <= LONGEST_ICON
            && !named
                .chars()
                .any(|letter| letter.is_whitespace() || letter.is_control());
        plain.then(|| Self(named.into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Icon {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shown {
    Application,
    Track,
    Album,
}

impl Shown {
    pub const ALL: [Self; 3] = [Self::Application, Self::Track, Self::Album];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Application => "application",
            Self::Track => "track",
            Self::Album => "album",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Application => "Just the player",
            Self::Track => "Track",
            Self::Album => "Track and album",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|shown| shown.as_str() == text)
    }
}

impl fmt::Display for Shown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pictured {
    Cover,
    Icon,
    Nothing,
}

impl Pictured {
    pub const ALL: [Self; 3] = [Self::Cover, Self::Icon, Self::Nothing];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cover => "cover",
            Self::Icon => "icon",
            Self::Nothing => "none",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Cover => "Cover",
            Self::Icon => "Icon",
            Self::Nothing => "None",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|pictured| pictured.as_str() == text)
    }
}

impl fmt::Display for Pictured {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Presence {
    pub enabled: bool,
    pub app: Option<AppId>,
    pub shown: Shown,
    pub pictured: Pictured,
    pub icon: Option<Icon>,
    pub progress: bool,
    pub while_paused: bool,
}

impl Presence {
    pub const OFF: Self = Self {
        enabled: false,
        app: None,
        shown: Shown::Album,
        pictured: Pictured::Cover,
        icon: None,
        progress: true,
        while_paused: false,
    };

    pub const fn active(&self) -> bool {
        self.enabled && self.app.is_some()
    }
}

impl Default for Presence {
    fn default() -> Self {
        Self::OFF
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_shown_by_default_and_nothing_is_shown_without_an_application() {
        assert!(!Presence::default().active());

        let switched_on = Presence {
            enabled: true,
            ..Presence::OFF
        };
        assert!(!switched_on.active());

        let named = Presence {
            app: AppId::parse("1234567890123456789"),
            ..Presence::OFF
        };
        assert!(!named.active());

        let both = Presence {
            enabled: true,
            ..named
        };
        assert!(both.active());
    }

    #[test]
    fn an_application_id_is_a_run_of_digits_as_long_as_a_snowflake() {
        assert_eq!(
            AppId::parse(" 1234567890123456789 ").map(AppId::get),
            Some(1_234_567_890_123_456_789)
        );
        assert_eq!(
            AppId::parse("12345678901234567").map(AppId::get),
            Some(12_345_678_901_234_567)
        );
        assert_eq!(AppId::parse(""), None);
        assert_eq!(AppId::parse("1234567890123456"), None);
        assert_eq!(AppId::parse("123456789012345678901"), None);
        assert_eq!(AppId::parse("12345678901234567a9"), None);
        assert_eq!(AppId::parse("+234567890123456789"), None);
        assert_eq!(AppId::parse("99999999999999999999"), None);
    }

    #[test]
    fn an_application_id_is_written_as_the_digits_it_was_read_from() {
        let id = AppId::parse("1234567890123456789").expect("a snowflake");
        assert_eq!(id.to_string(), "1234567890123456789");
    }

    #[test]
    fn an_icon_is_one_plain_word_or_address() {
        assert_eq!(
            Icon::parse(" resonate ").map(|icon| icon.to_string()),
            Some("resonate".to_owned())
        );
        assert!(Icon::parse("https://example.org/icon.png").is_some());
        assert_eq!(Icon::parse("   "), None);
        assert_eq!(Icon::parse("two words"), None);
        assert_eq!(Icon::parse(&"a".repeat(LONGEST_ICON + 1)), None);
    }

    #[test]
    fn every_shown_and_pictured_reads_back_from_the_name_it_is_written_under() {
        for shown in Shown::ALL {
            assert_eq!(Shown::parse(shown.as_str()), Some(shown));
        }
        for pictured in Pictured::ALL {
            assert_eq!(Pictured::parse(pictured.as_str()), Some(pictured));
        }
        assert_eq!(Shown::parse("everything"), None);
        assert_eq!(Pictured::parse("portrait"), None);
    }
}
