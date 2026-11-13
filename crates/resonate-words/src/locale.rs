#[cfg(not(any(windows, target_os = "macos")))]
const LANGUAGES_LISTED_IN: &str = "LANGUAGE";
#[cfg(not(any(windows, target_os = "macos")))]
const LOCALE_NAMED_IN: [&str; 3] = ["LC_ALL", "LC_MESSAGES", "LANG"];
const THE_PLAIN_LOCALES: [&str; 2] = ["c", "posix"];

#[cfg(not(any(windows, target_os = "macos")))]
pub fn languages_the_listener_reads() -> Vec<String> {
    let named = |variable: &str| {
        std::env::var(variable)
            .ok()
            .filter(|value| !value.is_empty())
    };
    let locale = LOCALE_NAMED_IN.into_iter().find_map(named);

    read_from(named(LANGUAGES_LISTED_IN).as_deref(), locale.as_deref())
}

#[cfg(any(windows, target_os = "macos"))]
pub fn languages_the_listener_reads() -> Vec<String> {
    let mut languages: Vec<String> = Vec::new();
    for tag in sys_locale::get_locales().filter_map(|locale| tagged(&locale)) {
        if !languages.contains(&tag) {
            languages.push(tag);
        }
    }
    languages
}

#[cfg(not(any(windows, target_os = "macos")))]
fn read_from(listed: Option<&str>, locale: Option<&str>) -> Vec<String> {
    let plain = locale.and_then(tagged).is_none();
    let listed = listed.filter(|_| !plain);

    let mut languages: Vec<String> = Vec::new();
    for tag in listed
        .into_iter()
        .flat_map(|listed| listed.split(':'))
        .chain(locale)
        .filter_map(tagged)
    {
        if !languages.contains(&tag) {
            languages.push(tag);
        }
    }
    languages
}

pub(crate) fn tagged(locale: &str) -> Option<String> {
    let tag = locale.split(['.', '@']).next()?.trim().replace('_', "-");
    let primary = tag.split('-').next()?.to_ascii_lowercase();

    (!primary.is_empty() && !THE_PLAIN_LOCALES.contains(&primary.as_str())).then_some(tag)
}

#[cfg(all(test, not(any(windows, target_os = "macos"))))]
mod tests {
    use super::*;

    #[test]
    fn a_locale_is_read_as_a_language_tag_and_the_list_comes_first() {
        assert_eq!(
            read_from(Some("pt_BR:ja"), Some("en_US.UTF-8")),
            ["pt-BR", "ja", "en-US"]
        );
        assert_eq!(read_from(None, Some("de_AT.UTF-8@euro")), ["de-AT"]);
        assert_eq!(read_from(Some("en:en"), Some("en")), ["en"]);
    }

    #[test]
    fn the_plain_locale_reads_no_language_and_silences_the_list() {
        assert!(read_from(Some("ja"), Some("C")).is_empty());
        assert!(read_from(Some("ja"), Some("POSIX")).is_empty());
        assert!(read_from(Some("ja"), None).is_empty());
    }
}
