#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::Path};

    const CLICKED: &str = ".on_click(";

    const NAMES_A_ROLE: [&str; 20] = [
        "said_as(",
        "named_as(",
        "listed_as(",
        "spoken_as(",
        "chosen_among(",
        "linked_as(",
        "pictured_as(",
        "dialog(",
        ".role(",
        "Role::",
        "kit::button(",
        "kit::icon_button(",
        "kit::segment(",
        "kit::pick(",
        "kit::radio_row(",
        "kit::checkbox(",
        "kit::star(",
        "kit::choice_row(",
        "kit::dot_swatch(",
        "kit::play_on_the_art(",
    ];

    const HAS_AN_ID: [&str; 5] = [
        ".id(",
        "kit::button(",
        "kit::icon_button(",
        "kit::segment(",
        "kit::play_on_the_art(",
    ];

    const STILL_POINTER_ONLY: &[&str] = &[
        "browser.rs | cell.cursor_pointer() | .on_click(cx.listener(move |this, _, _, cx| {",
        "browser.rs | row.cursor_pointer() | .on_click(cx.listener(move |this, _, _, cx| {",
        "browser.rs | row.cursor_pointer() | .on_click(cx.listener(move |this, _, _, cx| {",
        "chrome.rs | button | .on_click(move |_, window, _| control.act(window))",
        "dropping.rs | if stopping { | button.on_click(cx.listener(|this, _, _, cx| this.stop_copying(cx)))",
        "hint.rs | div() | .on_click(|_, _, cx| cx.stop_propagation())",
        "playlists.rs | .child( | control(\"pin-saved\", id, pin_icon(pinned), pin_hint(pinned)).on_click(",
        "playlists.rs | control( | .on_click(cx.listener(",
        "playlists.rs | control( | .on_click(cx.listener(move |this, _, _, cx| {",
        "playlists.rs | control( | .on_click(cx.listener(move |this, _, _, cx| {",
        "playlists.rs | control( | .on_click(cx.listener(move |this, _, _, cx| {",
        "playlists.rs | control( | .on_click(cx.listener(move |this, _, _, cx| {",
        "playlists.rs | control(\"play-saved\", id, Icon::Play, words::playlists::play_hint()) | .on_click(cx.listener(move |this, _, _, cx| {",
        "playlists.rs | div() | .on_click(cx.listener(move |this, _, window, cx| {",
        "playlists.rs | kit::framed_field(\"playlist-name\", true) | .on_click(cx.listener(|this, _, window, cx| {",
        "playlists.rs | let cell = div() | .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {",
        "playlists.rs | let listed = tall_row(playing) | .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {",
        "root/heading.rs | div() | .on_click(cx.listener(|this, _, window, cx| {",
        "root/heading.rs | let box_of_words = div() | .on_click(cx.listener(|this, _, window, cx| {",
        "root/pointer.rs | named | .on_click(cx.listener(move |this, _, _, cx| {",
        "served.rs | kit::framed_field(SEARCH_ID, false) | .on_click(cx.listener(|this, _, window, cx| {",
        "settings/library.rs | control | .on_click(cx.listener(move |this, click: &ClickEvent, _, cx| {",
        "settings/mod.rs | control | .on_click(cx.listener(move |this, click: &ClickEvent, window, cx| {",
        "settings/mod.rs | control | .on_click(cx.listener(move |this, click: &ClickEvent, window, cx| {",
        "settings/mod.rs | kit::framed_field(\"find-a-setting\", self.finding_holds_the_caret) | .on_click(cx.listener(|this, _, window, cx| {",
        "settings/mod.rs | kit::framed_field(id, false) | .on_click(cx.listener(move |_, _: &ClickEvent, window, cx| {",
        "settings/mod.rs | let clicked = press.clone(); | Self::ringed_for_keys(handle, control, press, cx).on_click(cx.listener(",
        "settings/output.rs | row.track_focus(&self.controls.at(reached, cx)) | .on_click(cx.listener(move |this, click: &ClickEvent, _, cx| {",
        "settings/sync.rs | div() | .on_click(cx.listener(|this, _, _, cx| this.show_the_pairing(true, cx)))",
    ];

    fn indent(line: &str) -> usize {
        line.len() - line.trim_start().len()
    }

    fn sources() -> Vec<(String, String)> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/views");
        let mut read = Vec::new();
        walk(&root, &root, &mut read);
        assert!(
            read.len() > 20,
            "the walk found {} view files, so it is not reading the views at all",
            read.len()
        );
        read
    }

    fn walk(root: &Path, folder: &Path, read: &mut Vec<(String, String)>) {
        for entry in fs::read_dir(folder).expect("the views folder is readable") {
            let path = entry.expect("a readable directory entry").path();
            if path.is_dir() {
                walk(root, &path, read);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                let name = path
                    .strip_prefix(root)
                    .expect("a file under the views")
                    .to_string_lossy()
                    .into_owned();
                read.push((
                    name,
                    fs::read_to_string(&path).expect("a readable source file"),
                ));
            }
        }
    }

    fn built_part(source: &str) -> Vec<&str> {
        source
            .lines()
            .take_while(|line| *line != "#[cfg(test)]")
            .collect()
    }

    fn chain_before<'a>(lines: &[&'a str], at: usize) -> Vec<&'a str> {
        let click = indent(lines[at]);
        let mut start = at;
        while start > 0 {
            let above = lines[start - 1];
            let trimmed = above.trim_start();
            let continues = trimmed.is_empty()
                || indent(above) > click
                || (indent(above) == click && continues_the_chain(trimmed));
            if !continues {
                break;
            }
            start -= 1;
        }
        let base = start.saturating_sub(1);

        lines[base..=at].to_vec()
    }

    fn continues_the_chain(trimmed: &str) -> bool {
        trimmed.starts_with('.') || trimmed.starts_with(')') || trimmed.starts_with('}')
    }

    fn lacks_what_a_reader_needs(chain: &[&str]) -> bool {
        let text = chain.join("\n");
        let id = HAS_AN_ID.iter().any(|marker| text.contains(marker));
        let role = NAMES_A_ROLE.iter().any(|marker| text.contains(marker));

        !(id && role)
    }

    fn pointer_only_sites() -> Vec<String> {
        let mut found = Vec::new();
        for (name, source) in sources() {
            let lines = built_part(&source);
            for (at, line) in lines.iter().enumerate() {
                if !line.contains(CLICKED) {
                    continue;
                }
                let chain = chain_before(&lines, at);
                if lacks_what_a_reader_needs(&chain) {
                    found.push(format!("{name} | {} | {}", chain[0].trim(), line.trim()));
                }
            }
        }
        found.sort();
        found
    }

    fn counted(sites: &[String]) -> BTreeMap<&str, usize> {
        let mut counts = BTreeMap::new();
        for site in sites {
            *counts.entry(site.as_str()).or_insert(0) += 1;
        }
        counts
    }

    #[test]
    fn every_click_a_view_listens_for_is_on_an_element_a_reader_can_name() {
        let found = pointer_only_sites();
        let allowed: Vec<String> = STILL_POINTER_ONLY
            .iter()
            .map(|site| (*site).to_owned())
            .collect();
        let found_counts = counted(&found);
        let allowed_counts = counted(&allowed);

        for (site, count) in &found_counts {
            assert!(
                allowed_counts
                    .get(site)
                    .is_some_and(|allowed| allowed >= count),
                "a click is listened for on an element with no id or no role, so a reader cannot \
                 reach it: {site}"
            );
        }
        for (site, count) in &allowed_counts {
            assert!(
                found_counts.get(site).is_some_and(|found| found >= count),
                "this site is named in the allow-list but no longer pointer-only, so remove it: {site}"
            );
        }
    }

    #[test]
    #[ignore = "prints the sites to seed the allow-list"]
    fn list_the_sites() {
        for site in pointer_only_sites() {
            println!("        {site:?},");
        }
    }
}

#[cfg(test)]
mod words {
    use std::path::Path;

    const PROPER: &[&str] = &[
        "AcoustID",
        "Android",
        "AudD",
        "AutoEq",
        "Clean",
        "Core Audio",
        "Discord",
        "Explicit",
        "Jellyfin",
        "Last.fm",
        "Linux",
        "Lipshitz",
        "ListenBrainz",
        "PipeWire",
        "Resonate",
        "Shazam",
        "Subsonic",
        "Windows",
        "YouTube Music",
        "macOS",
    ];

    const NOT_WORDS: &[&str] = &[
        "fonts.rs",
        "icons.rs",
        "launcher.rs",
        "wrapped_card/markup.rs",
    ];

    const ONLY_UNDER_TEST: &[&str] = &["models/tests.rs", "views/root/tests.rs"];

    const STILL_TO_MOVE: &[&str] = &[];

    #[test]
    fn every_word_the_window_shows_is_said_through_its_catalog() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let found =
            resonate_words_build::prose_in(&root, PROPER).expect("the window's sources read");

        let unmoved: Vec<String> = found
            .iter()
            .filter(|(path, _)| {
                let path = path.to_string_lossy();
                !STILL_TO_MOVE.contains(&path.as_ref())
                    && !NOT_WORDS.contains(&path.as_ref())
                    && !ONLY_UNDER_TEST.contains(&path.as_ref())
            })
            .flat_map(|(path, prose)| {
                prose
                    .iter()
                    .map(move |said| format!("{}:{} {:?}", path.display(), said.line, said.text))
            })
            .collect();
        let moved: Vec<&&str> = STILL_TO_MOVE
            .iter()
            .filter(|path| !found.keys().any(|found| found.to_string_lossy() == **path))
            .collect();

        assert!(
            unmoved.is_empty(),
            "words written in the code:\n{}",
            unmoved.join("\n")
        );
        assert!(
            moved.is_empty(),
            "files whose words are all moved: {moved:?}"
        );
    }
}
