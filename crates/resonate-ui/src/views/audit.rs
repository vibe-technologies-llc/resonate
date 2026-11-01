#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs, path::Path};

    const CLICKED: &str = ".on_click(";

    const NAMES_A_ROLE: [&str; 17] = [
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
        "kit::chip(",
        "kit::star(",
        "kit::choice_row(",
        "kit::dot_swatch(",
    ];

    const HAS_AN_ID: [&str; 4] = [".id(", "kit::button(", "kit::icon_button(", "kit::segment("];

    const STILL_POINTER_ONLY: &[&str] = &[];

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
                || (indent(above) == click && trimmed.starts_with('.'));
            if !continues {
                break;
            }
            start -= 1;
        }
        let base = start.saturating_sub(1);

        lines[base..=at].to_vec()
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
