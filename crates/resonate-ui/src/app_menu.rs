use gpui::{App, Menu, MenuItem, OsAction, SystemMenuType};

use crate::{
    app::{
        About, CloseTheWindow, CycleRepeat, FocusFilter, FocusSearch, Hide, HideOthers, Listen,
        Minimise, Next, Previous, Quit, ShowAll, Stop, ToggleFullScreen, ToggleMute,
        TogglePlayPause, ToggleQueue, ToggleShuffle, VolumeDown, VolumeUp, WINDOW_TITLE, Zoom,
    },
    views::field::{CopySelection, CutSelection, PasteClipboard, Redo, SelectEverything, Undo},
    words,
};

pub(crate) fn lay_out(cx: &mut App) {
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.set_menus(menus());
}

fn menus() -> Vec<Menu> {
    vec![
        the_apps_own(),
        editing(),
        playback(),
        viewing(),
        the_windows(),
    ]
}

fn the_apps_own() -> Menu {
    Menu::new(WINDOW_TITLE).items([
        MenuItem::action(words::app_menu::about(), About),
        MenuItem::separator(),
        MenuItem::action(words::app_menu::settings(), FocusFilter),
        MenuItem::separator(),
        MenuItem::os_submenu(words::app_menu::services(), SystemMenuType::Services),
        MenuItem::separator(),
        MenuItem::action(words::app_menu::hide(), Hide),
        MenuItem::action(words::app_menu::hide_others(), HideOthers),
        MenuItem::action(words::app_menu::show_all(), ShowAll),
        MenuItem::separator(),
        MenuItem::action(words::app_menu::quit(), Quit),
    ])
}

fn editing() -> Menu {
    Menu::new(words::app_menu::edit()).items([
        MenuItem::os_action(words::app_menu::undo(), Undo, OsAction::Undo),
        MenuItem::os_action(words::app_menu::redo(), Redo, OsAction::Redo),
        MenuItem::separator(),
        MenuItem::os_action(words::app_menu::cut(), CutSelection, OsAction::Cut),
        MenuItem::os_action(words::app_menu::copy(), CopySelection, OsAction::Copy),
        MenuItem::os_action(words::app_menu::paste(), PasteClipboard, OsAction::Paste),
        MenuItem::os_action(
            words::app_menu::select_all(),
            SelectEverything,
            OsAction::SelectAll,
        ),
    ])
}

fn playback() -> Menu {
    Menu::new(words::app_menu::playback()).items([
        MenuItem::action(words::app_menu::play_or_pause(), TogglePlayPause),
        MenuItem::action(words::app_menu::stop(), Stop),
        MenuItem::separator(),
        MenuItem::action(words::app_menu::next(), Next),
        MenuItem::action(words::app_menu::previous(), Previous),
        MenuItem::separator(),
        MenuItem::action(words::app_menu::shuffle(), ToggleShuffle),
        MenuItem::action(words::app_menu::repeat(), CycleRepeat),
        MenuItem::separator(),
        MenuItem::action(words::app_menu::louder(), VolumeUp),
        MenuItem::action(words::app_menu::quieter(), VolumeDown),
        MenuItem::action(words::app_menu::mute(), ToggleMute),
    ])
}

fn viewing() -> Menu {
    Menu::new(words::app_menu::view()).items([
        MenuItem::action(words::app_menu::queue(), ToggleQueue),
        MenuItem::action(words::app_menu::search(), FocusSearch),
        MenuItem::action(words::app_menu::listen(), Listen),
        MenuItem::separator(),
        MenuItem::action(words::app_menu::full_screen(), ToggleFullScreen),
    ])
}

fn the_windows() -> Menu {
    Menu::new(words::app_menu::window()).items([
        MenuItem::action(words::app_menu::minimise(), Minimise),
        MenuItem::action(words::app_menu::zoom(), Zoom),
        MenuItem::separator(),
        MenuItem::action(words::app_menu::close(), CloseTheWindow),
    ])
}

#[cfg(test)]
mod tests {
    use gpui::{Action, Keymap, MenuItem, OperatingSystem};

    use super::menus;
    use crate::{app, words};

    const THE_NAME_GPUI_LISTS_THE_WINDOWS_UNDER: &str = "Window";

    fn asked_of_the_window() -> Vec<Box<dyn Action>> {
        menus()
            .into_iter()
            .flat_map(|menu| menu.items)
            .filter_map(|item| match item {
                MenuItem::Action { action, .. } => Some(action),
                MenuItem::Separator | MenuItem::Submenu(_) | MenuItem::SystemMenu(_) => None,
            })
            .collect()
    }

    #[test]
    fn the_menu_a_mac_lists_its_windows_under_is_named_as_gpui_looks_for_it() {
        assert!(
            menus()
                .iter()
                .any(|menu| menu.name == THE_NAME_GPUI_LISTS_THE_WINDOWS_UNDER)
        );
    }

    #[test]
    fn no_item_of_the_menu_is_asked_twice() {
        let asked: Vec<&'static str> = asked_of_the_window()
            .iter()
            .map(|action| action.name())
            .collect();

        for (place, name) in asked.iter().enumerate() {
            assert!(
                !asked[..place].contains(name),
                "{name} stands in the menu twice"
            );
        }
    }

    #[test]
    fn every_item_of_the_playback_and_edit_menus_shows_a_key_on_a_mac() {
        let keymap = Keymap::new(app::bindings(OperatingSystem::Mac));

        for menu in menus().into_iter().filter(|menu| {
            menu.name == words::app_menu::playback() || menu.name == words::app_menu::edit()
        }) {
            for item in menu.items {
                if let MenuItem::Action { action, .. } = item {
                    assert!(
                        keymap.bindings_for_action(action.as_ref()).next().is_some(),
                        "{} stands in the menu with no key beside it",
                        action.name()
                    );
                }
            }
        }
    }

    #[test]
    fn a_key_the_menu_shows_beside_an_item_is_never_one_a_listener_types() {
        let keymap = Keymap::new(app::bindings(OperatingSystem::Mac));

        for action in asked_of_the_window() {
            for binding in keymap.bindings_for_action(action.as_ref()) {
                assert!(
                    binding.keystrokes().iter().all(|stroke| {
                        stroke.modifiers().platform || stroke.modifiers().control
                    }),
                    "{} is bound to a bare key, which the menu would take from a field being \
                     typed into",
                    action.name()
                );
            }
        }
    }
}
