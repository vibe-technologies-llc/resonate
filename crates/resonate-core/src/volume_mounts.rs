use std::path::{Path, PathBuf};

const VOLUME_WORD: &str = "volume";
const GUID_GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
const GROUP_SEPARATOR: char = '-';

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct VolumeGuid(String);

impl VolumeGuid {
    pub fn within(text: &str) -> Option<Self> {
        let lowered = text.to_ascii_lowercase();
        let after_the_word = lowered.find(VOLUME_WORD)? + VOLUME_WORD.len();
        let opened = lowered.get(after_the_word..)?.strip_prefix('{')?;
        let (guid, _) = opened.split_once('}')?;
        let groups: Vec<&str> = guid.split(GROUP_SEPARATOR).collect();
        let shaped = groups.len() == GUID_GROUPS.len()
            && groups.iter().zip(GUID_GROUPS).all(|(group, width)| {
                group.len() == width && group.bytes().all(|byte| byte.is_ascii_hexdigit())
            });
        shaped.then(|| Self(guid.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn nearest_mounted_volume(
    path: &Path,
    reparse_target_of: impl FnMut(&Path) -> Option<PathBuf>,
) -> Option<VolumeGuid> {
    path.ancestors()
        .filter_map(reparse_target_of)
        .find_map(|target| VolumeGuid::within(&target.to_string_lossy()))
}

pub fn identity_of(prefix: &str, mounted: Option<VolumeGuid>) -> String {
    mounted.or_else(|| VolumeGuid::within(prefix)).map_or_else(
        || prefix.to_ascii_uppercase(),
        |guid| format!("{VOLUME_WORD}{{{}}}", guid.as_str()),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    const GUID: &str = "4a8f3c1e-0b7d-4e52-9a61-c3d2e5f60718";

    fn guid() -> VolumeGuid {
        VolumeGuid::within(&format!(r"\\?\Volume{{{GUID}}}\")).expect("a volume name")
    }

    #[test]
    fn a_volume_is_named_by_the_guid_every_spelling_of_its_target_carries() {
        for spelled in [
            format!(r"\\?\Volume{{{GUID}}}\"),
            format!(r"\??\Volume{{{GUID}}}\"),
            format!(r"\\?\VOLUME{{{}}}\Music", GUID.to_ascii_uppercase()),
            format!(r"\\.\Volume{{{GUID}}}"),
        ] {
            assert_eq!(VolumeGuid::within(&spelled), Some(guid()), "{spelled}");
        }
        assert_eq!(guid().as_str(), GUID);
    }

    #[test]
    fn a_target_that_names_no_volume_names_none() {
        for spelled in [
            r"D:\Stuff",
            r"\\?\D:\Stuff",
            r"\\server\share\Music",
            r"\\?\Volume{not-a-guid}\",
            r"\\?\Volume{4a8f3c1e-0b7d-4e52-9a61-c3d2e5f6071}\",
            r"\\?\Volume{4a8f3c1e-0b7d-4e52-9a61-c3d2e5f6071g}\",
            r"\\?\Volume{4a8f3c1e-0b7d-4e52-9a61-c3d2e5f60718",
            "",
        ] {
            assert_eq!(VolumeGuid::within(spelled), None, "{spelled}");
        }
    }

    #[test]
    fn the_nearest_ancestor_that_is_a_mounted_volume_names_the_volume() {
        let outer = "11111111-2222-3333-4444-555555555555";
        let mounts: BTreeMap<PathBuf, PathBuf> = [
            (
                PathBuf::from("/Mounts/Stick"),
                PathBuf::from(format!(r"\\?\Volume{{{GUID}}}\")),
            ),
            (
                PathBuf::from("/Mounts"),
                PathBuf::from(format!(r"\\?\Volume{{{outer}}}\")),
            ),
            (PathBuf::from("/Junction"), PathBuf::from(r"D:\Stuff")),
        ]
        .into_iter()
        .collect();
        let target = |ancestor: &Path| mounts.get(ancestor).cloned();

        assert_eq!(
            nearest_mounted_volume(Path::new("/Mounts/Stick/Music/a.flac"), target),
            Some(guid())
        );
        assert_eq!(
            nearest_mounted_volume(Path::new("/Mounts/Stick"), target),
            Some(guid())
        );
        assert_eq!(
            nearest_mounted_volume(Path::new("/Junction/Music"), target),
            None
        );
        assert_eq!(
            nearest_mounted_volume(Path::new("/Other/a.flac"), target),
            None
        );
    }

    #[test]
    fn a_mounted_folder_outranks_the_drive_it_sits_on_and_a_drive_is_named_by_its_prefix() {
        assert_eq!(
            identity_of(r"\\?\C:", Some(guid())),
            format!("volume{{{GUID}}}")
        );
        assert_eq!(identity_of(r"\\?\c:", None), r"\\?\C:");
        assert_eq!(
            identity_of(&format!(r"\\?\Volume{{{GUID}}}"), None),
            identity_of(r"\\?\C:", Some(guid()))
        );
    }
}
