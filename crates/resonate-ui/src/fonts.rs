use std::{
    borrow::Cow,
    env, fs,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use gpui::SharedString;
use parking_lot::RwLock;

const THE_BUNDLE_CARRIES_ITS_FONTS: bool = cfg!(target_os = "macos");

const FROM_THE_EXECUTABLES_FOLDER_TO_THE_FONTS: &str = "../Resources/Fonts";

const FONT_FILES: [&str; 3] = ["ttf", "otf", "ttc"];

const BODY_LADDER: [&str; 11] = [
    "Inter",
    "Adwaita Sans",
    "Cantarell",
    "Noto Sans",
    "Open Sans",
    "Source Sans 3",
    "Liberation Sans",
    "DejaVu Sans",
    "Segoe UI Variable",
    "Segoe UI",
    "Helvetica Neue",
];

const MONOSPACE_LADDER: [&str; 11] = [
    "JetBrains Mono",
    "Adwaita Mono",
    "Fira Mono",
    "Source Code Pro",
    "Hack",
    "Noto Sans Mono",
    "Liberation Mono",
    "DejaVu Sans Mono",
    "Cascadia Mono",
    "Consolas",
    "Menlo",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Faces {
    pub body: SharedString,
    pub monospace: SharedString,
}

impl Faces {
    fn wanted() -> Self {
        Self {
            body: SharedString::new_static(BODY_LADDER[0]),
            monospace: SharedString::new_static(MONOSPACE_LADDER[0]),
        }
    }
}

static DRAWN_IN: LazyLock<RwLock<Faces>> = LazyLock::new(|| RwLock::new(Faces::wanted()));

pub fn bundled() -> Vec<Cow<'static, [u8]>> {
    if !THE_BUNDLE_CARRIES_ITS_FONTS {
        return Vec::new();
    }
    env::current_exe()
        .ok()
        .as_deref()
        .and_then(Path::parent)
        .map(|beside| fonts_in(&beside.join(FROM_THE_EXECUTABLES_FOLDER_TO_THE_FONTS)))
        .unwrap_or_default()
}

fn fonts_in(folder: &Path) -> Vec<Cow<'static, [u8]>> {
    let Ok(held) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = held
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|file| is_a_font_file(file))
        .collect();
    files.sort();

    files
        .iter()
        .filter_map(|file| match fs::read(file) {
            Ok(bytes) => Some(Cow::Owned(bytes)),
            Err(error) => {
                tracing::warn!(file = %file.display(), %error, "a font the application carries could not be read");
                None
            }
        })
        .collect()
}

fn is_a_font_file(file: &Path) -> bool {
    file.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            FONT_FILES
                .iter()
                .any(|font| extension.eq_ignore_ascii_case(font))
        })
}

pub fn settle(installed: &[String]) {
    *DRAWN_IN.write() = Faces {
        body: among(&BODY_LADDER, installed),
        monospace: among(&MONOSPACE_LADDER, installed),
    };
}

pub fn drawn_in() -> Faces {
    DRAWN_IN.read().clone()
}

pub fn body() -> SharedString {
    DRAWN_IN.read().body.clone()
}

pub fn monospace() -> SharedString {
    DRAWN_IN.read().monospace.clone()
}

fn among(ladder: &[&'static str], installed: &[String]) -> SharedString {
    ladder
        .iter()
        .find_map(|wanted| held(wanted, installed))
        .unwrap_or_else(|| SharedString::new_static(ladder[0]))
}

fn held(wanted: &str, installed: &[String]) -> Option<SharedString> {
    installed
        .iter()
        .find(|family| family.eq_ignore_ascii_case(wanted))
        .cloned()
        .map(SharedString::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(families: &[&str]) -> Vec<String> {
        families.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn the_fonts_a_bundle_carries_are_read_in_name_order_and_nothing_else_is() {
        let folder = env::temp_dir().join(format!("resonate-fonts-{}", std::process::id()));
        fs::create_dir_all(&folder).expect("a folder for the fonts");
        fs::write(folder.join("b.ttf"), b"second").expect("a font file");
        fs::write(folder.join("a.OTF"), b"first").expect("a font file");
        fs::write(folder.join("licence.txt"), b"words").expect("a file that is no font");

        let read = fonts_in(&folder);
        fs::remove_dir_all(&folder).expect("the folder is taken away again");

        assert_eq!(
            read,
            [Cow::Borrowed(&b"first"[..]), Cow::Borrowed(&b"second"[..])]
        );
    }

    #[test]
    fn a_bundle_with_no_fonts_folder_carries_none() {
        let nowhere = env::temp_dir().join(format!("resonate-no-fonts-{}", std::process::id()));

        assert!(fonts_in(&nowhere).is_empty());
    }

    #[test]
    fn a_mac_with_neither_face_installed_still_finds_one_the_system_has() {
        let on_a_mac = installed(&["Helvetica Neue", "Menlo", "Times"]);

        assert_eq!(among(&BODY_LADDER, &on_a_mac).as_ref(), "Helvetica Neue");
        assert_eq!(among(&MONOSPACE_LADDER, &on_a_mac).as_ref(), "Menlo");
    }

    #[test]
    fn the_first_installed_family_on_the_ladder_is_the_one_drawn_in() {
        let found = among(&BODY_LADDER, &installed(&["Cantarell", "Noto Sans"]));

        assert_eq!(found.as_ref(), "Cantarell");
    }

    #[test]
    fn a_family_is_answered_with_the_spelling_the_machine_has_it_under() {
        let found = among(&MONOSPACE_LADDER, &installed(&["jetbrains mono"]));

        assert_eq!(found.as_ref(), "jetbrains mono");
    }

    #[test]
    fn a_machine_with_none_of_them_keeps_the_family_that_was_wanted() {
        let found = among(&BODY_LADDER, &installed(&["Comic Sans MS"]));

        assert_eq!(found.as_ref(), BODY_LADDER[0]);
    }

    #[test]
    fn neither_ladder_offers_a_family_the_other_one_does() {
        for body in BODY_LADDER {
            assert!(
                !MONOSPACE_LADDER.contains(&body),
                "{body} stands on both ladders, so a body face could be taken for a mono one"
            );
        }
    }

    #[test]
    fn nothing_is_settled_until_the_window_says_what_is_installed() {
        assert_eq!(Faces::wanted().body.as_ref(), BODY_LADDER[0]);
        assert_eq!(Faces::wanted().monospace.as_ref(), MONOSPACE_LADDER[0]);
    }
}
