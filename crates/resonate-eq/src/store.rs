use std::{
    fmt::Write as _,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use resonate_core::eq::Profile;

use crate::{Error, Result, StoreOp, apo, graphic};

pub const EXTENSION: &str = "txt";
pub const NAME_AT_MOST: usize = 96;
pub const PROFILES_AT_MOST: usize = 256;
pub const OWN_FOLDER: &str = "own";

const STAGED_EXTENSION: &str = "txt.new";
const FILE_NAME_AT_MOST: usize = 255;
const EVERY_OTHER_DEVICE: &str = "every-other-device";
const A_DEVICE: &str = "device-";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProfileName(Box<str>);

impl ProfileName {
    pub fn new(text: &str) -> Result<Self> {
        let text = text.trim();
        let usable = !text.is_empty()
            && text.len() <= NAME_AT_MOST
            && text != "."
            && text != ".."
            && !text.contains(['/', '\\', '\0'])
            && !text.chars().any(char::is_control);

        if !usable {
            return Err(Error::NameNotUsable);
        }
        Ok(Self(text.into()))
    }

    pub fn after(label: &str) -> Self {
        let mut spelled = String::with_capacity(label.len());
        for glyph in label.chars() {
            if glyph == '/' || glyph == '\\' || glyph == '\0' || glyph.is_control() {
                spelled.push('-');
            } else {
                spelled.push(glyph);
            }
        }
        let spelled: String = spelled.split_whitespace().collect::<Vec<_>>().join(" ");
        let cut: String = spelled.chars().take(NAME_AT_MOST).collect();
        Self::new(&cut).unwrap_or_else(|_| Self("profile".into()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn folded(&self) -> String {
        self.0.to_lowercase()
    }
}

impl std::fmt::Display for ProfileName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Binding {
    Profile(ProfileName),
    Own,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Kept {
    pub profile: Profile,
    pub converted: bool,
    pub passed_over: usize,
}

pub struct Store {
    folder: PathBuf,
}

impl Store {
    pub const fn at(folder: PathBuf) -> Self {
        Self { folder }
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn path_of(&self, name: &ProfileName) -> PathBuf {
        self.folder.join(format!("{name}.{EXTENSION}"))
    }

    fn failed(path: &Path, op: StoreOp) -> impl FnOnce(std::io::Error) -> Error + use<'_> {
        move |source| Error::Store {
            path: path.to_path_buf(),
            op,
            source,
        }
    }

    pub fn names(&self) -> Result<Vec<ProfileName>> {
        let walked = match fs::read_dir(&self.folder) {
            Ok(walked) => walked,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(Self::failed(&self.folder, StoreOp::Walk)(source)),
        };

        let mut names = Vec::new();
        for entry in walked.flatten() {
            if names.len() >= PROFILES_AT_MOST {
                tracing::debug!(
                    folder = %self.folder.display(),
                    "more profiles than this build lists"
                );
                break;
            }
            let path = entry.path();
            if path.extension().is_none_or(|held| held != EXTENSION) {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str())
                && let Ok(name) = ProfileName::new(stem)
            {
                names.push(name);
            }
        }
        names.sort();
        Ok(names)
    }

    fn text_of(path: &Path) -> Result<Option<String>> {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(Self::failed(path, StoreOp::Read)(source)),
        };

        let mut text = String::new();
        file.take(apo::LARGEST_PROFILE as u64 + 1)
            .read_to_string(&mut text)
            .map_err(Self::failed(path, StoreOp::Read))?;
        Ok(Some(text))
    }

    pub fn read(&self, name: &ProfileName) -> Result<Option<Profile>> {
        let path = self.path_of(name);
        let Some(text) = Self::text_of(&path)? else {
            return Ok(None);
        };
        Ok(Some(apo::read_profile(&text)?))
    }

    pub fn keep(&self, name: &ProfileName, profile: &Profile) -> Result<PathBuf> {
        let path = self.path_of(name);
        Self::staged(&path, profile)?;
        Ok(path)
    }

    fn staged(path: &Path, profile: &Profile) -> Result<()> {
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder).map_err(Self::failed(folder, StoreOp::Write))?;
        }
        let staged = path.with_extension(STAGED_EXTENSION);
        fs::write(&staged, apo::write(profile))
            .and_then(|()| fs::rename(&staged, path))
            .map_err(Self::failed(path, StoreOp::Write))
    }

    pub fn own_path(&self, device: Option<&str>) -> Result<PathBuf> {
        let stem = match device {
            Some(device) => stem_for_a_device(device)?,
            None => EVERY_OTHER_DEVICE.to_owned(),
        };
        Ok(self
            .folder
            .join(OWN_FOLDER)
            .join(format!("{stem}.{EXTENSION}")))
    }

    pub fn own(&self, device: Option<&str>) -> Result<Profile> {
        let path = self.own_path(device)?;
        match Self::text_of(&path)? {
            Some(text) => Ok(apo::read_profile(&text)?),
            None => Ok(Profile::flat()),
        }
    }

    pub fn keep_own(&self, device: Option<&str>, profile: &Profile) -> Result<PathBuf> {
        let path = self.own_path(device)?;
        Self::staged(&path, profile)?;
        Ok(path)
    }

    pub fn forget(&self, name: &ProfileName) -> Result<bool> {
        let path = self.path_of(name);
        match fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(Self::failed(&path, StoreOp::Remove)(source)),
        }
    }

    pub fn read_in(from: &Path) -> Result<Kept> {
        let Some(text) = Self::text_of(from)? else {
            return Err(Error::Store {
                path: from.to_path_buf(),
                op: StoreOp::Read,
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            });
        };

        if let Some(curve) = graphic::read(&text)? {
            return Ok(Kept {
                profile: graphic::bands_fitted_to(&curve),
                converted: true,
                passed_over: 0,
            });
        }

        let reading = apo::read(&text)?;
        if reading.profile.bands().is_empty() && reading.profile.preamp().is_none() {
            return Err(Error::NotAProfile {
                path: from.to_path_buf(),
            });
        }
        Ok(Kept {
            profile: reading.profile,
            converted: false,
            passed_over: reading.passed_over,
        })
    }

    pub fn import(&self, from: &Path, called: Option<&ProfileName>) -> Result<(ProfileName, Kept)> {
        let kept = Self::read_in(from)?;
        let name = match called {
            Some(name) => name.clone(),
            None => from
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map_or_else(|| ProfileName::after("profile"), ProfileName::after),
        };
        self.keep(&name, &kept.profile)?;
        Ok((name, kept))
    }

    pub fn export(&self, profile: &Profile, to: &Path) -> Result<()> {
        fs::write(to, apo::write(profile)).map_err(Self::failed(to, StoreOp::Write))
    }
}

fn stem_for_a_device(device: &str) -> Result<String> {
    let mut stem = String::with_capacity(A_DEVICE.len() + device.len());
    stem.push_str(A_DEVICE);
    for byte in device.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-') {
            stem.push(char::from(byte));
        } else {
            let _ = write!(stem, "%{byte:02X}");
        }
    }

    let staged_length = stem.len() + 1 + STAGED_EXTENSION.len();
    if device.is_empty() || staged_length > FILE_NAME_AT_MOST {
        return Err(Error::DeviceNotNameable);
    }
    Ok(stem)
}

#[cfg(test)]
mod tests {
    use std::{
        env, process,
        sync::atomic::{AtomicU64, Ordering},
    };

    use resonate_core::eq::{Band, BandGain, BandKind, Frequency, Preamp, Q};

    use super::*;

    struct Scratch {
        store: Store,
    }

    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let folder = env::temp_dir().join(format!(
                "resonate-eq-{}-{}",
                process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            Self {
                store: Store::at(folder),
            }
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(self.store.folder());
        }
    }

    fn named(text: &str) -> ProfileName {
        ProfileName::new(text).expect("a usable name")
    }

    fn profile() -> Profile {
        Profile::new(
            Preamp::from_decibels(-6.1).expect("in range"),
            vec![Band::new(
                BandKind::LowShelf,
                Frequency::from_hertz(105.0).expect("in range"),
                BandGain::from_decibels(6.4).expect("in range"),
                Q::from_units(0.7).expect("in range"),
            )],
        )
        .expect("one band")
    }

    #[test]
    fn a_profile_kept_reads_back_as_the_profile_it_was() {
        let scratch = Scratch::new();
        let name = named("Sennheiser HD 650");

        scratch
            .store
            .keep(&name, &profile())
            .expect("a writable folder");
        assert_eq!(
            scratch.store.read(&name).expect("it reads"),
            Some(profile())
        );
        assert_eq!(scratch.store.names().expect("it walks"), vec![name.clone()]);

        assert!(scratch.store.forget(&name).expect("it removes"));
        assert_eq!(scratch.store.read(&name).expect("it reads"), None);
        assert!(!scratch.store.forget(&name).expect("nothing to remove"));
    }

    #[test]
    fn a_folder_that_is_not_there_holds_no_profiles_rather_than_failing() {
        let scratch = Scratch::new();

        assert!(scratch.store.names().expect("it walks").is_empty());
        assert_eq!(
            scratch.store.read(&named("nothing")).expect("it reads"),
            None
        );
    }

    #[test]
    fn a_name_that_would_leave_the_folder_is_refused() {
        assert!(ProfileName::new("").is_err());
        assert!(ProfileName::new("   ").is_err());
        assert!(ProfileName::new(".").is_err());
        assert!(ProfileName::new("..").is_err());
        assert!(ProfileName::new("a/b").is_err());
        assert!(ProfileName::new("a\\b").is_err());
        assert!(ProfileName::new(&"x".repeat(NAME_AT_MOST + 1)).is_err());
        assert!(ProfileName::new("Sennheiser HD 650").is_ok());

        assert_eq!(ProfileName::after("a/b\\c").as_str(), "a-b-c");
        assert_eq!(
            ProfileName::after("  spaced   out  ").as_str(),
            "spaced out"
        );
    }

    #[test]
    fn a_parametric_file_imports_as_the_bands_it_names() {
        let scratch = Scratch::new();
        let folder = scratch.store.folder().to_path_buf();
        fs::create_dir_all(&folder).expect("a writable folder");
        let from = folder.join("measured.txt");
        fs::write(&from, apo::write(&profile())).expect("a writable file");

        let (name, kept) = scratch.store.import(&from, None).expect("it imports");

        assert_eq!(name.as_str(), "measured");
        assert!(!kept.converted);
        assert_eq!(kept.profile, profile());
        assert_eq!(
            scratch.store.read(&name).expect("it reads"),
            Some(profile())
        );
    }

    #[test]
    fn a_graphic_file_imports_as_a_conversion_and_says_so() {
        let scratch = Scratch::new();
        let folder = scratch.store.folder().to_path_buf();
        fs::create_dir_all(&folder).expect("a writable folder");
        let from = folder.join("curve.txt");
        fs::write(&from, "GraphicEQ: 20 -9; 200 -9; 2000 0; 20000 0").expect("a writable file");

        let (name, kept) = scratch
            .store
            .import(&from, Some(&named("Bassy")))
            .expect("it imports");

        assert_eq!(name.as_str(), "Bassy");
        assert!(kept.converted);
        assert!(!kept.profile.bands().is_empty());
    }

    #[test]
    fn a_file_that_is_neither_is_refused_rather_than_kept_empty() {
        let scratch = Scratch::new();
        let folder = scratch.store.folder().to_path_buf();
        fs::create_dir_all(&folder).expect("a writable folder");
        let from = folder.join("nonsense.txt");
        fs::write(&from, "the quick brown fox\n").expect("a writable file");

        assert!(matches!(
            scratch.store.import(&from, None),
            Err(Error::NotAProfile { .. })
        ));
    }

    #[test]
    fn a_profile_exported_is_a_file_another_player_reads() {
        let scratch = Scratch::new();
        let folder = scratch.store.folder().to_path_buf();
        fs::create_dir_all(&folder).expect("a writable folder");
        let to = folder.join("exported.txt");

        scratch.store.export(&profile(), &to).expect("it writes");
        let text = fs::read_to_string(&to).expect("it reads back");

        assert!(text.starts_with("Preamp:"), "{text}");
        assert!(text.contains("Filter 1: ON LSC Fc 105 Hz"), "{text}");
        assert_eq!(apo::read_profile(&text).expect("it parses"), profile());
    }

    #[test]
    fn a_device_that_never_shaped_its_own_curve_holds_a_flat_one() {
        let scratch = Scratch::new();

        assert_eq!(
            scratch
                .store
                .own(Some("alsa_output.usb"))
                .expect("it reads"),
            Profile::flat()
        );
        assert_eq!(scratch.store.own(None).expect("it reads"), Profile::flat());
    }

    #[test]
    fn an_own_curve_kept_reads_back_for_its_device_and_for_no_other() {
        let scratch = Scratch::new();
        let device = "alsa_output.usb-Sennheiser_HD_650.analog-stereo";

        scratch
            .store
            .keep_own(Some(device), &profile())
            .expect("a writable folder");

        assert_eq!(
            scratch.store.own(Some(device)).expect("it reads"),
            profile()
        );
        assert_eq!(
            scratch.store.own(None).expect("it reads"),
            Profile::flat(),
            "one device's curve reached every other device"
        );
        assert_eq!(
            scratch
                .store
                .own(Some("alsa_output.pci"))
                .expect("it reads"),
            Profile::flat()
        );
        assert!(
            scratch.store.names().expect("it walks").is_empty(),
            "an own curve was listed as a kept profile"
        );
    }

    #[test]
    fn no_device_name_reaches_the_file_another_device_or_the_rest_are_kept_in() {
        let scratch = Scratch::new();
        let own = |device: Option<&str>| scratch.store.own_path(device).expect("a usable name");

        let spellings = [
            Some("a/b"),
            Some("a%2Fb"),
            Some("a%252Fb"),
            Some("every-other-device"),
            Some(".."),
            Some("."),
            Some("a b"),
            Some("a_b"),
            None,
        ];
        let paths: Vec<PathBuf> = spellings.into_iter().map(own).collect();
        for (at, path) in paths.iter().enumerate() {
            assert_eq!(
                path.parent(),
                Some(scratch.store.folder().join(OWN_FOLDER).as_path()),
                "{} left the folder",
                path.display()
            );
            for other in paths.iter().skip(at + 1) {
                assert_ne!(path, other, "two owners share {}", path.display());
            }
        }

        assert!(matches!(
            scratch.store.own_path(Some("")),
            Err(Error::DeviceNotNameable)
        ));
        assert!(matches!(
            scratch.store.own_path(Some(&"/".repeat(FILE_NAME_AT_MOST))),
            Err(Error::DeviceNotNameable)
        ));
    }

    #[test]
    fn a_crash_part_way_through_a_write_cannot_truncate_a_profile() {
        let scratch = Scratch::new();
        let name = named("held");
        scratch
            .store
            .keep(&name, &profile())
            .expect("a writable folder");

        let staged = scratch.store.path_of(&name).with_extension("txt.new");
        assert!(!staged.exists(), "the staged file outlived the rename");
    }
}
