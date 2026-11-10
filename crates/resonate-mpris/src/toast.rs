use std::fmt::Write as _;

use crate::notify::{Shown, actions};

const LINES_UNDER_THE_TITLE_AT_MOST: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Buttons {
    Offered,
    Withheld,
}

pub(crate) fn document(shown: &Shown, buttons: Buttons) -> String {
    let mut xml =
        String::from(r#"<toast duration="short"><visual><binding template="ToastGeneric">"#);

    if let Some(art) = shown.art.as_deref() {
        let _ = write!(
            xml,
            r#"<image placement="appLogoOverride" src="{}"/>"#,
            escaped(art)
        );
    }
    let _ = write!(xml, "<text>{}</text>", escaped(&shown.summary));
    for line in lines_under_the_title(&shown.body) {
        let _ = write!(xml, "<text>{}</text>", escaped(line));
    }
    xml.push_str("</binding></visual>");

    if buttons == Buttons::Offered {
        xml.push_str("<actions>");
        for [key, label] in actions().as_chunks::<2>().0 {
            let _ = write!(
                xml,
                r#"<action content="{}" arguments="{}" activationType="foreground"/>"#,
                escaped(label),
                escaped(key)
            );
        }
        xml.push_str("</actions>");
    }

    xml.push_str(r#"<audio silent="true"/></toast>"#);
    xml
}

fn lines_under_the_title(body: &str) -> Vec<&str> {
    body.splitn(LINES_UNDER_THE_TITLE_AT_MOST, '\n')
        .filter(|line| !line.is_empty())
        .collect()
}

fn escaped(text: &str) -> String {
    let mut held = String::with_capacity(text.len());
    for character in text
        .chars()
        .filter(|character| is_xml_character(*character))
    {
        match character {
            '&' => held.push_str("&amp;"),
            '<' => held.push_str("&lt;"),
            '>' => held.push_str("&gt;"),
            '"' => held.push_str("&quot;"),
            '\'' => held.push_str("&apos;"),
            other => held.push(other),
        }
    }
    held
}

const fn is_xml_character(character: char) -> bool {
    matches!(
        character,
        '\t' | '\n' | '\r' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echoes() -> Shown {
        Shown::told(
            "Echoes".to_owned(),
            "Pink Floyd\nMeddle".to_owned(),
            Some("file:///C:/Users/a/AppData/Local/Temp/resonate-art/1.png".to_owned()),
        )
    }

    #[test]
    fn a_toast_says_the_title_over_the_artist_and_the_album_with_the_cover_beside() {
        let xml = document(&echoes(), Buttons::Withheld);

        assert!(xml.contains("<text>Echoes</text><text>Pink Floyd</text><text>Meddle</text>"));
        assert!(xml.contains(
            r#"<image placement="appLogoOverride" src="file:///C:/Users/a/AppData/Local/Temp/resonate-art/1.png"/>"#
        ));
        assert!(xml.ends_with(r#"<audio silent="true"/></toast>"#));
    }

    #[test]
    fn a_toast_with_no_cover_names_no_image_and_one_with_no_body_no_second_line() {
        let shown = Shown::told("Echoes".to_owned(), String::new(), None);
        let xml = document(&shown, Buttons::Withheld);

        assert!(!xml.contains("<image"));
        assert_eq!(xml.matches("<text>").count(), 1);
    }

    #[test]
    fn each_button_a_toast_offers_is_a_transport_key_pressed_in_the_window() {
        let xml = document(&echoes(), Buttons::Offered);

        for (key, label) in [
            ("previous", "Previous"),
            ("play-pause", "Play/Pause"),
            ("next", "Next"),
        ] {
            assert!(xml.contains(&format!(
                r#"<action content="{label}" arguments="{key}" activationType="foreground"/>"#
            )));
        }
        assert_eq!(xml.matches("<action ").count(), 3);
    }

    #[test]
    fn a_toast_told_without_buttons_carries_no_actions() {
        assert!(!document(&echoes(), Buttons::Withheld).contains("<actions>"));
    }

    #[test]
    fn what_a_tag_holds_that_xml_refuses_is_escaped_or_left_out() {
        let shown = Shown::told(
            "Simon & \"Garfunkel\" <live> 'x'\u{0}\u{8}".to_owned(),
            String::new(),
            None,
        );
        let xml = document(&shown, Buttons::Withheld);

        assert!(
            xml.contains(
                "<text>Simon &amp; &quot;Garfunkel&quot; &lt;live&gt; &apos;x&apos;</text>"
            )
        );
    }

    #[test]
    fn more_than_two_lines_under_the_title_fold_into_the_second() {
        assert_eq!(lines_under_the_title("a\nb\nc"), vec!["a", "b\nc"]);
        assert!(lines_under_the_title("").is_empty());
    }
}
