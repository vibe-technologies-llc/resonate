use std::{
    collections::BinaryHeap,
    ffi::OsStr,
    fmt::Write as _,
    fs,
    io::{Read, Write as _},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use resonate_core::{
    eq::Profile,
    naming::{self, NAME_BYTES_AT_MOST},
    text::{self, TextEncoding, UTF8_BOM},
    writer::{self, Held, Writer},
};

use crate::{
    BuiltIn, EqOp, Error, Fitted, Fitting, LARGEST_MEASUREMENT, Layout, Response, ResponseReading,
    Result, StoreOp, apo, fit, read_response,
};

pub const EXTENSION: &str = "txt";
pub const NAME_AT_MOST: usize = 96;
pub const PROFILES_AT_MOST: usize = 256;
pub const OWN_FOLDER: &str = "own";

const STAGED_SUFFIX: &str = ".new";
const HIDDEN: &str = ".";
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
        Self::new(cut_to_fit(&spelled)).unwrap_or_else(|_| Self("profile".into()))
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
    pub approximated: usize,
    pub clamped: usize,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Measured {
    pub fitted: Fitted,
    pub layout: Layout,
    pub passed_over: usize,
}

struct Standing {
    text: String,
    encoding: TextEncoding,
    marked_as_utf8: bool,
}

impl Standing {
    fn of(bytes: Vec<u8>) -> Self {
        let (text, encoding) = text::decoded(&bytes);
        let marked_as_utf8 = encoding == TextEncoding::Utf8 && bytes.starts_with(&UTF8_BOM);
        Self {
            text,
            encoding,
            marked_as_utf8,
        }
    }

    fn spelling(&self, text: &str) -> Vec<u8> {
        let Some(body) = text::encoded(text, self.encoding) else {
            tracing::debug!(
                encoding = %self.encoding,
                "a profile with a letter its code page lacks is kept as UTF-8"
            );
            return text.as_bytes().to_vec();
        };
        let mark: &[u8] = if self.marked_as_utf8 {
            &UTF8_BOM
        } else {
            self.encoding.byte_order_mark()
        };
        [mark, &body].concat()
    }
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

        let names = walked.flatten().filter_map(|entry| {
            let path = entry.path();
            if path.extension().is_none_or(|held| held != EXTENSION) {
                return None;
            }
            let stem = path.file_stem()?.to_str()?;
            ProfileName::new(stem).ok()
        });
        Ok(the_least(names, &self.folder))
    }

    fn bytes_up_to(path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
        let file = match fs::File::open(path) {
            Ok(file) => file,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(Self::failed(path, StoreOp::Read)(source)),
        };

        let mut bytes = Vec::new();
        file.take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(Self::failed(path, StoreOp::Read))?;
        Ok(Some(bytes))
    }

    fn bytes_of(path: &Path) -> Result<Option<Vec<u8>>> {
        let Some(bytes) = Self::bytes_up_to(path, apo::LARGEST_PROFILE)? else {
            return Ok(None);
        };
        if bytes.len() > apo::LARGEST_PROFILE {
            return Err(Error::TooLarge {
                op: EqOp::Parse,
                limit: apo::LARGEST_PROFILE,
            });
        }
        Ok(Some(bytes))
    }

    fn text_of(path: &Path) -> Result<Option<String>> {
        Ok(Self::bytes_of(path)?.map(|bytes| text::decoded(&bytes).0))
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
        Self::revised(&path, profile)?;
        Ok(path)
    }

    pub fn keep_read(&self, name: &ProfileName, kept: &Kept) -> Result<PathBuf> {
        let path = self.path_of(name);
        Self::staged(&path, &kept.bytes)?;
        Ok(path)
    }

    fn rendered(over: Option<&Path>, profile: &Profile) -> Vec<u8> {
        let standing = over
            .and_then(|path| Self::bytes_of(path).ok().flatten())
            .map(Standing::of);
        match standing {
            Some(standing) => match apo::edited(&standing.text, profile) {
                Some(edited) => standing.spelling(&edited),
                None => apo::write(profile).into_bytes(),
            },
            None => apo::write(profile).into_bytes(),
        }
    }

    fn revised(path: &Path, profile: &Profile) -> Result<()> {
        let rendered = Self::rendered(Some(path), profile);
        if Self::bytes_of(path).ok().flatten().as_deref() == Some(rendered.as_slice()) {
            return Ok(());
        }
        Self::staged(path, &rendered)
    }

    fn staged(path: &Path, bytes: &[u8]) -> Result<()> {
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder).map_err(Self::failed(folder, StoreOp::Write))?;
            sweep_staged_in(folder);
        }
        let staged = staging_name_for(path);
        Self::written_and_synced(&staged, path, bytes).map_err(Self::failed(path, StoreOp::Write))
    }

    fn written_and_synced(staged: &Path, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let landed = Held::made(staged).and_then(|held| {
            let mut file = held.file();
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(staged, path)?;
            match path
                .parent()
                .filter(|folder| !folder.as_os_str().is_empty())
            {
                Some(folder) => fs::File::open(folder)?.sync_all(),
                None => Ok(()),
            }
        });
        if landed.is_err() {
            let _ = fs::remove_file(staged);
        }
        landed
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
        Self::revised(&path, profile)?;
        Ok(path)
    }

    pub fn keep_own_read(&self, device: Option<&str>, kept: &Kept) -> Result<PathBuf> {
        let path = self.own_path(device)?;
        Self::staged(&path, &kept.bytes)?;
        Ok(path)
    }

    pub fn file_of(&self, owner: Option<&str>, binding: &Binding) -> Result<PathBuf> {
        match binding {
            Binding::Profile(name) => Ok(self.path_of(name)),
            Binding::Own => self.own_path(owner),
        }
    }

    pub fn owners(&self) -> Result<Vec<Option<String>>> {
        let folder = self.folder.join(OWN_FOLDER);
        let walked = match fs::read_dir(&folder) {
            Ok(walked) => walked,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(source) => return Err(Self::failed(&folder, StoreOp::Walk)(source)),
        };

        let owners = walked.flatten().filter_map(|entry| {
            let path = entry.path();
            if path.extension().is_none_or(|held| held != EXTENSION) {
                return None;
            }
            let stem = path.file_stem()?.to_str()?;
            if stem == EVERY_OTHER_DEVICE {
                Some(None)
            } else {
                device_of_a_stem(stem).map(Some)
            }
        });
        Ok(the_least(owners, &folder))
    }

    pub fn forget(&self, name: &ProfileName) -> Result<bool> {
        Self::removed(&self.path_of(name))
    }

    pub fn forget_own(&self, device: Option<&str>) -> Result<bool> {
        Self::removed(&self.own_path(device)?)
    }

    fn removed(path: &Path) -> Result<bool> {
        match fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(source) => Err(Self::failed(path, StoreOp::Remove)(source)),
        }
    }

    pub fn read_in(from: &Path) -> Result<Kept> {
        let Some(bytes) = Self::bytes_of(from)? else {
            return Err(Error::Store {
                path: from.to_path_buf(),
                op: StoreOp::Read,
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            });
        };

        let reading = apo::read(&text::decoded(&bytes).0)?;
        if reading.profile.is_transparent() && reading.profile.target().is_none() {
            return Err(Error::NotAProfile {
                path: from.to_path_buf(),
            });
        }
        Ok(Kept {
            converted: reading.profile.target().is_some(),
            profile: reading.profile,
            passed_over: reading.passed_over,
            approximated: reading.approximated,
            clamped: reading.clamped,
            bytes,
        })
    }

    pub fn response_in(from: &Path) -> Result<ResponseReading> {
        let Some(bytes) = Self::bytes_up_to(from, LARGEST_MEASUREMENT)? else {
            return Err(Self::failed(from, StoreOp::Read)(std::io::Error::from(
                std::io::ErrorKind::NotFound,
            )));
        };
        read_response(&bytes)
    }

    pub fn goal_named(text: &str) -> Result<Response> {
        match BuiltIn::parse(text) {
            Some(built_in) => built_in.response(),
            None => Ok(Self::response_in(Path::new(text))?.response),
        }
    }

    pub fn fit_file(from: &Path, goal: &Response, fitting: &Fitting) -> Result<Measured> {
        let reading = Self::response_in(from)?;
        let fitted = fit(&reading.response, goal, fitting)?;
        Ok(Measured {
            fitted,
            layout: reading.layout,
            passed_over: reading.passed_over,
        })
    }

    pub fn holds(&self, name: &ProfileName) -> bool {
        self.path_of(name).exists()
    }

    pub fn name_for_import(from: &Path, called: Option<&ProfileName>) -> ProfileName {
        match called {
            Some(name) => name.clone(),
            None => from
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map_or_else(|| ProfileName::after("profile"), ProfileName::after),
        }
    }

    pub fn import(&self, from: &Path, called: Option<&ProfileName>) -> Result<(ProfileName, Kept)> {
        let kept = Self::read_in(from)?;
        let name = Self::name_for_import(from, called);
        self.keep_read(&name, &kept)?;
        Ok((name, kept))
    }

    pub fn export(&self, profile: &Profile, standing_in: Option<&Path>, to: &Path) -> Result<()> {
        let rendered = Self::rendered(standing_in, profile);
        let staged = staging_name_for(to);
        Self::written_and_synced(&staged, to, &rendered).map_err(Self::failed(to, StoreOp::Write))
    }
}

fn staging_name_for(path: &Path) -> PathBuf {
    static STAGED: AtomicU64 = AtomicU64::new(0);

    let stamped = format!(
        ".{}-{}{STAGED_SUFFIX}",
        process::id(),
        STAGED.fetch_add(1, Ordering::Relaxed)
    );
    let name = path.file_name().unwrap_or_else(|| OsStr::new("profile"));
    path.with_file_name(naming::named_within(HIDDEN, name, stamped))
}

fn stamp_of_staged(name: &str) -> Option<u32> {
    let stamped = name.strip_prefix(HIDDEN)?.strip_suffix(STAGED_SUFFIX)?;
    let (_, tail) = stamped.rsplit_once('.')?;
    let (pid, counter) = tail.split_once('-')?;
    counter.parse::<u64>().ok()?;
    pid.parse().ok()
}

fn sweep_staged_in(folder: &Path) {
    let Ok(walked) = fs::read_dir(folder) else {
        return;
    };
    for entry in walked.flatten() {
        let path = entry.path();
        let Some(stamp) = path
            .file_name()
            .and_then(OsStr::to_str)
            .and_then(stamp_of_staged)
        else {
            continue;
        };
        if writer::writer_of(&path, stamp) == Writer::Gone {
            let _ = fs::remove_file(&path);
        }
    }
}

fn cut_to_fit(spelled: &str) -> &str {
    let fits = spelled
        .char_indices()
        .map(|(at, glyph)| at + glyph.len_utf8())
        .take_while(|&end| end <= NAME_AT_MOST)
        .last()
        .unwrap_or(0);
    &spelled[..fits]
}

fn the_least<T: Ord>(walked: impl Iterator<Item = T>, folder: &Path) -> Vec<T> {
    let mut least = BinaryHeap::with_capacity(PROFILES_AT_MOST + 1);
    let mut passed_over = 0_usize;
    for item in walked {
        least.push(item);
        if least.len() > PROFILES_AT_MOST {
            least.pop();
            passed_over += 1;
        }
    }

    if passed_over > 0 {
        tracing::debug!(
            folder = %folder.display(),
            passed_over,
            "more files than this build lists"
        );
    }
    least.into_sorted_vec()
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

    let file_length = stem.len() + 1 + EXTENSION.len();
    if device.is_empty() || file_length > NAME_BYTES_AT_MOST {
        return Err(Error::DeviceNotNameable);
    }
    Ok(stem)
}

fn device_of_a_stem(stem: &str) -> Option<String> {
    let mut rest = stem.strip_prefix(A_DEVICE)?.as_bytes();
    let mut bytes = Vec::with_capacity(rest.len());
    while let Some((&byte, after)) = rest.split_first() {
        if byte == b'%' {
            let digits = std::str::from_utf8(after.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(digits, 16).ok()?);
            rest = after.get(2..)?;
        } else {
            bytes.push(byte);
            rest = after;
        }
    }

    let device = String::from_utf8(bytes).ok()?;
    let written_the_same_way = stem_for_a_device(&device).is_ok_and(|again| again == stem);
    written_the_same_way.then_some(device)
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
    fn a_name_cut_to_fit_keeps_as_many_letters_as_the_bytes_allow_in_any_script() {
        let wide = "\u{97f3}".repeat(40);
        let cut = ProfileName::after(&wide);

        assert_eq!(cut.as_str(), "\u{97f3}".repeat(NAME_AT_MOST / 3));
        assert_ne!(ProfileName::after(&"\u{3042}".repeat(40)), cut);
        assert_eq!(
            ProfileName::after(&format!("{}\u{97f3}", "x".repeat(NAME_AT_MOST - 1))).as_str(),
            "x".repeat(NAME_AT_MOST - 1)
        );
        assert_eq!(
            ProfileName::after(&"x".repeat(200)).as_str(),
            "x".repeat(NAME_AT_MOST)
        );
    }

    #[test]
    fn the_profiles_listed_past_the_limit_are_the_first_by_name_whatever_the_folder_order() {
        let scratch = Scratch::new();
        let folder = scratch.store.folder().to_path_buf();
        fs::create_dir_all(&folder).expect("a writable folder");
        let held = PROFILES_AT_MOST + 20;
        for at in (0..held).rev() {
            fs::write(folder.join(format!("p{at:04}.txt")), "Preamp: -1 dB").expect("writable");
        }

        let listed = scratch.store.names().expect("it walks");
        let expected: Vec<ProfileName> = (0..PROFILES_AT_MOST)
            .map(|at| named(&format!("p{at:04}")))
            .collect();

        assert_eq!(listed, expected);
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
    fn a_utf16_file_past_the_bytes_a_profile_holds_is_refused_rather_than_read_in_part() {
        let scratch = Scratch::new();
        let folder = scratch.store.folder().to_path_buf();
        fs::create_dir_all(&folder).expect("a writable folder");
        let from = folder.join("wide.txt");
        let text = "Filter 1: ON PK Fc 1000 Hz Gain 1 dB Q 1\n".repeat(1_000);
        let bytes: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        fs::write(&from, bytes).expect("a writable file");

        assert!(matches!(Store::read_in(&from), Err(Error::TooLarge { .. })));
    }

    #[test]
    fn a_store_says_whether_a_name_is_kept_before_an_import_takes_it() {
        let scratch = Scratch::new();
        let folder = scratch.store.folder().to_path_buf();
        fs::create_dir_all(&folder).expect("a writable folder");
        let source = folder.join("source");
        fs::create_dir_all(&source).expect("a writable folder");
        let from = source.join("measured.txt");
        fs::write(&from, apo::write(&profile())).expect("a writable file");

        let name = Store::name_for_import(&from, None);
        assert_eq!(name.as_str(), "measured");
        assert!(!scratch.store.holds(&name));

        scratch.store.import(&from, None).expect("it imports");
        assert!(scratch.store.holds(&name));
        assert_eq!(
            Store::name_for_import(&from, Some(&named("other"))),
            named("other")
        );
    }

    #[test]
    fn a_file_a_windows_editor_saved_as_utf_16_imports_as_the_bands_it_names() {
        let scratch = Scratch::new();
        let folder = scratch.store.folder().to_path_buf();
        fs::create_dir_all(&folder).expect("a writable folder");
        let from = folder.join("measured.txt");
        let mut wide = vec![0xFF, 0xFE];
        for unit in apo::write(&profile()).encode_utf16() {
            wide.extend_from_slice(&unit.to_le_bytes());
        }
        fs::write(&from, wide).expect("a writable file");

        let (_, kept) = scratch.store.import(&from, None).expect("it imports");

        assert_eq!(kept.profile, profile());
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
    fn a_graphic_file_is_kept_as_its_curve_and_reads_back_able_to_fit_again() {
        let scratch = Scratch::new();
        let folder = scratch.store.folder().to_path_buf();
        fs::create_dir_all(&folder).expect("a writable folder");
        let from = folder.join("curve.txt");
        fs::write(&from, "GraphicEQ: 20 -9; 200 -9; 2000 0; 20000 0").expect("a writable file");

        let (name, kept) = scratch.store.import(&from, None).expect("it imports");
        let read = scratch
            .store
            .read(&name)
            .expect("it reads")
            .expect("it was kept");

        assert!(read.target().is_some());
        assert_eq!(read, kept.profile);

        let mut shaped = read;
        if let Some(band) = shaped.band_mut(0) {
            band.on = false;
        }
        scratch.store.keep(&name, &shaped).expect("it keeps");
        let reshaped = scratch
            .store
            .read(&name)
            .expect("it reads")
            .expect("it was kept");
        assert_eq!(reshaped.target(), None);
        assert_eq!(reshaped, shaped);
    }

    #[test]
    fn a_preamp_written_beside_a_graphic_curve_is_the_one_it_plays_at() {
        let scratch = Scratch::new();
        let folder = scratch.store.folder().to_path_buf();
        fs::create_dir_all(&folder).expect("a writable folder");
        let from = folder.join("curve.txt");
        fs::write(
            &from,
            "Preamp: -4.5 dB\nGraphicEQ: 20 -9; 200 -9; 2000 0; 20000 0\n",
        )
        .expect("a writable file");

        let (_, kept) = scratch.store.import(&from, None).expect("it imports");

        assert_eq!(kept.profile.preamp().milli_decibels(), -4_500);
        assert!(kept.profile.target().is_some());
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

        scratch
            .store
            .export(&profile(), None, &to)
            .expect("it writes");
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
            scratch
                .store
                .own_path(Some(&"/".repeat(NAME_BYTES_AT_MOST))),
            Err(Error::DeviceNotNameable)
        ));
    }

    #[test]
    fn every_own_curve_kept_is_listed_under_the_device_it_was_kept_for() {
        let scratch = Scratch::new();
        let devices = [
            "alsa_output.usb-Sennheiser_HD_650.analog-stereo",
            "a/b",
            "a%2Fb",
            "every-other-device",
            "..",
            "Kopfhörer",
        ];
        for device in devices {
            scratch
                .store
                .keep_own(Some(device), &profile())
                .expect("a writable folder");
        }
        scratch
            .store
            .keep_own(None, &profile())
            .expect("a writable folder");

        let mut expected: Vec<Option<String>> = devices
            .into_iter()
            .map(|device| Some(device.to_owned()))
            .chain([None])
            .collect();
        expected.sort();
        assert_eq!(scratch.store.owners().expect("it walks"), expected);
    }

    #[test]
    fn a_file_no_device_is_written_as_is_not_listed_as_an_own_curve() {
        let scratch = Scratch::new();
        let own = scratch.store.folder().join(OWN_FOLDER);
        fs::create_dir_all(&own).expect("a writable folder");
        for stray in [
            "device-a%2fb.txt",
            "device-a%+1.txt",
            "device-a%2.txt",
            "device-a b.txt",
            "device-.txt",
            "someone-else.txt",
            "device-held.txt.new",
        ] {
            fs::write(own.join(stray), "Preamp: -1 dB").expect("a writable folder");
        }

        assert!(
            scratch.store.owners().expect("it walks").is_empty(),
            "a file this build would never write was read as a device's curve"
        );
    }

    #[test]
    fn an_own_curve_forgotten_leaves_its_device_flat_and_every_other_curve_standing() {
        let scratch = Scratch::new();
        let device = "alsa_output.usb";
        scratch
            .store
            .keep_own(Some(device), &profile())
            .expect("a writable folder");
        scratch
            .store
            .keep_own(None, &profile())
            .expect("a writable folder");

        assert!(scratch.store.forget_own(Some(device)).expect("it removes"));
        assert!(
            !scratch.store.forget_own(Some(device)).expect("it removes"),
            "a curve already gone was forgotten again"
        );
        assert_eq!(
            scratch.store.own(Some(device)).expect("it reads"),
            Profile::flat()
        );
        assert_eq!(scratch.store.own(None).expect("it reads"), profile());
        assert_eq!(scratch.store.owners().expect("it walks"), vec![None]);
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

    const ANNOTATED: &str = "# a hand-edited profile\r\n\
                             Include: shared.txt\r\n\
                             Preamp: -6.1 dB\r\n\
                             # the low end\r\n\
                             Filter 1: ON LSC Fc 105 Hz Gain 6.4 dB Q 0.70\r\n";

    fn folder_entries(store: &Store) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(store.folder())
            .expect("a folder")
            .flatten()
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect();
        names.sort();
        names
    }

    fn placed(scratch: &Scratch, name: &ProfileName, bytes: &[u8]) -> PathBuf {
        fs::create_dir_all(scratch.store.folder()).expect("a writable folder");
        let path = scratch.store.path_of(name);
        fs::write(&path, bytes).expect("a writable folder");
        path
    }

    fn louder() -> Profile {
        let mut louder = profile();
        louder.band_mut(0).expect("a band").gain = BandGain::from_decibels(7.0).expect("in range");
        louder
    }

    #[test]
    fn a_profile_kept_over_a_hand_edited_file_changes_the_lines_it_moved_and_nothing_else() {
        let scratch = Scratch::new();
        let name = named("hand edited");
        let path = placed(&scratch, &name, ANNOTATED.as_bytes());

        scratch.store.keep(&name, &louder()).expect("it keeps");

        assert_eq!(
            fs::read_to_string(&path).expect("it reads"),
            ANNOTATED
                .replace("Gain 6.4 dB", "Gain 7.000 dB")
                .replace(" Q 0.70", " Q 0.700")
        );
        assert_eq!(scratch.store.read(&name).expect("it reads"), Some(louder()));
    }

    #[test]
    fn a_profile_kept_unchanged_leaves_its_file_untouched() {
        let scratch = Scratch::new();
        let name = named("hand edited");
        let path = placed(&scratch, &name, ANNOTATED.as_bytes());
        let read = scratch.store.read(&name).expect("it reads").expect("kept");

        scratch.store.keep(&name, &read).expect("it keeps");

        assert_eq!(fs::read(&path).expect("it reads"), ANNOTATED.as_bytes());
        assert_eq!(folder_entries(&scratch.store), ["hand edited.txt"]);
    }

    #[test]
    fn a_file_in_a_wide_or_legacy_encoding_is_kept_in_it() {
        let scratch = Scratch::new();
        let body = "# Écoute\nPreamp: -6.1 dB\nFilter 1: ON LSC Fc 105 Hz Gain 6.4 dB Q 0.70\n";

        for (label, encoding, bom) in [
            ("sixteen little", TextEncoding::Utf16Le, true),
            ("sixteen big", TextEncoding::Utf16Be, true),
            ("windows", TextEncoding::WINDOWS_1252, false),
        ] {
            let name = named(label);
            let mut bytes = encoding.byte_order_mark().to_vec();
            bytes.extend(text::encoded(body, encoding).expect("every letter fits"));
            let path = placed(&scratch, &name, &bytes);

            scratch.store.keep(&name, &louder()).expect("it keeps");

            let kept = fs::read(&path).expect("it reads");
            let (decoded, found) = text::decoded(&kept);
            assert_eq!(found, encoding, "{label}");
            assert_eq!(
                kept.starts_with(encoding.byte_order_mark()) && bom,
                bom,
                "{label}"
            );
            assert!(decoded.contains("# Écoute"), "{label}");
            assert!(decoded.contains("Gain 7.000 dB"), "{label}");
        }
    }

    #[test]
    fn a_file_with_a_utf8_mark_keeps_the_mark() {
        let scratch = Scratch::new();
        let name = named("marked");
        let mut bytes = UTF8_BOM.to_vec();
        bytes.extend_from_slice(ANNOTATED.as_bytes());
        let path = placed(&scratch, &name, &bytes);

        scratch.store.keep(&name, &louder()).expect("it keeps");

        assert!(fs::read(&path).expect("it reads").starts_with(&UTF8_BOM));
    }

    #[test]
    fn text_with_a_letter_its_code_page_lacks_is_kept_as_utf8_rather_than_lost() {
        let scratch = Scratch::new();
        let name = named("latin");
        let path = placed(&scratch, &name, b"# caf\xe9\nPreamp: -6.1 dB\n");
        let mut with_a_letter = profile();
        with_a_letter.set_preamp(Preamp::from_decibels(-3.0).expect("in range"));

        scratch.store.keep(&name, &with_a_letter).expect("it keeps");

        assert!(
            text::decoded(&fs::read(&path).expect("it reads"))
                .0
                .contains("Preamp")
        );
    }

    #[test]
    fn an_import_keeps_the_file_it_read_comments_and_all() {
        let scratch = Scratch::new();
        let source = scratch.store.folder().join("elsewhere.txt");
        fs::create_dir_all(scratch.store.folder()).expect("a writable folder");
        fs::write(&source, ANNOTATED).expect("a writable folder");

        let (name, kept) = scratch.store.import(&source, None).expect("it imports");

        assert_eq!(
            fs::read_to_string(scratch.store.path_of(&name)).expect("it reads"),
            ANNOTATED
        );
        assert_eq!(kept.bytes, ANNOTATED.as_bytes());
    }

    #[test]
    fn an_import_over_a_profile_of_that_name_replaces_it_whole_rather_than_editing_it() {
        let scratch = Scratch::new();
        let name = named("elsewhere");
        placed(&scratch, &name, b"# the old one\nPreamp: -1 dB\n");
        let source = scratch.store.folder().join("elsewhere.txt.src");
        fs::write(&source, ANNOTATED).expect("a writable folder");

        scratch
            .store
            .import(&source, Some(&name))
            .expect("it imports");

        assert_eq!(
            fs::read_to_string(scratch.store.path_of(&name)).expect("it reads"),
            ANNOTATED
        );
    }

    #[test]
    fn an_export_is_the_kept_text_where_nothing_moved_and_is_staged_beside_its_target() {
        let scratch = Scratch::new();
        let name = named("hand edited");
        let kept = placed(&scratch, &name, ANNOTATED.as_bytes());
        let to = scratch.store.folder().join("out.txt");
        let read = scratch.store.read(&name).expect("it reads").expect("kept");

        scratch
            .store
            .export(&read, Some(&kept), &to)
            .expect("it exports");

        assert_eq!(fs::read(&to).expect("it reads"), ANNOTATED.as_bytes());
        assert_eq!(
            folder_entries(&scratch.store),
            ["hand edited.txt", "out.txt"]
        );

        scratch
            .store
            .export(&louder(), None, &to)
            .expect("it exports");

        assert_eq!(
            fs::read_to_string(&to).expect("it reads"),
            apo::write(&louder())
        );
        assert_eq!(
            folder_entries(&scratch.store),
            ["hand edited.txt", "out.txt"]
        );
    }

    #[test]
    fn two_writers_stage_under_names_of_their_own_and_a_dead_ones_leftover_is_swept() {
        let scratch = Scratch::new();
        let name = named("shared");
        let path = scratch.store.path_of(&name);
        fs::create_dir_all(scratch.store.folder()).expect("a writable folder");

        let first = staging_name_for(&path);
        let second = staging_name_for(&path);
        assert_ne!(first, second);
        assert_eq!(first.parent(), path.parent());

        let dead = scratch.store.folder().join(".shared.txt.999999999-0.new");
        fs::write(&dead, "half").expect("a writable folder");
        let living = scratch
            .store
            .folder()
            .join(format!(".other.txt.{}-77.new", process::id()));
        fs::write(&living, "half").expect("a writable folder");

        scratch.store.keep(&name, &profile()).expect("it keeps");

        assert!(!dead.exists(), "a dead writer's leftover stayed");
        assert!(living.exists(), "a living writer's file was taken");
        assert_eq!(
            folder_entries(&scratch.store),
            [
                format!(".other.txt.{}-77.new", process::id()),
                "shared.txt".to_owned()
            ]
        );
    }

    #[test]
    fn a_staged_name_for_a_file_at_the_name_limit_still_fits_the_filesystem() {
        let scratch = Scratch::new();
        let device = "x".repeat(NAME_BYTES_AT_MOST - A_DEVICE.len() - 1 - EXTENSION.len());

        let kept = scratch
            .store
            .keep_own(Some(&device), &profile())
            .expect("a device whose file fits keeps its curve");

        assert_eq!(kept.file_name().map(OsStr::len), Some(NAME_BYTES_AT_MOST));
        let staged = staging_name_for(&kept);
        assert!(staged.file_name().expect("a name").len() <= NAME_BYTES_AT_MOST);
        assert_eq!(
            scratch.store.own(Some(&device)).expect("it reads"),
            profile()
        );
    }

    #[test]
    fn the_staged_name_carries_the_pid_that_a_sweep_asks_after() {
        assert_eq!(stamp_of_staged(".shared.txt.4242-17.new"), Some(4242));
        assert_eq!(stamp_of_staged("shared.txt"), None);
        assert_eq!(stamp_of_staged(".shared.txt.new"), None);
        assert_eq!(stamp_of_staged(".a.b.7-x.new"), None);
    }

    fn response_text(shift_db: f64) -> String {
        (0..40)
            .map(|step| {
                let hertz = 20.0 * 1.2_f64.powi(step);
                let level = if (800.0..2_000.0).contains(&hertz) {
                    shift_db
                } else {
                    0.0
                };
                format!("{hertz:.2} {level:.2}\n")
            })
            .collect()
    }

    #[test]
    fn a_measurement_in_a_file_is_fitted_and_says_how_it_was_laid_out() {
        let scratch = Scratch::new();
        let folder = scratch.store.folder().to_path_buf();
        fs::create_dir_all(&folder).expect("a writable folder");
        let file = folder.join("measured.txt");
        let dip: String = (0..60)
            .map(|step| {
                let hertz = 20.0 * 1.12_f64.powi(step);
                let level = if (3_000.0..6_000.0).contains(&hertz) {
                    -5.0
                } else {
                    0.0
                };
                format!("{hertz:.2}\t{level:.2}\n")
            })
            .collect();
        fs::write(&file, format!("* REW\n{dip}")).expect("a writable folder");

        let measured = Store::fit_file(&file, &Response::flat(), &Fitting::default())
            .expect("the file is fitted");

        assert_eq!(measured.layout, Layout::Rew);
        assert!(!measured.fitted.profile.bands().is_empty());
        assert!(measured.fitted.off_by_db_after < measured.fitted.off_by_db_before);

        let name = named("fitted");
        scratch
            .store
            .keep(&name, &measured.fitted.profile)
            .expect("kept");
        assert_eq!(
            scratch.store.read(&name).expect("it reads"),
            Some(measured.fitted.profile)
        );
    }

    #[test]
    fn a_target_is_a_built_in_by_name_or_else_a_file_in_the_same_formats() {
        let scratch = Scratch::new();
        let folder = scratch.store.folder().to_path_buf();
        fs::create_dir_all(&folder).expect("a writable folder");
        let file = folder.join("mine.csv");
        fs::write(
            &file,
            format!("frequency,raw\n{}", response_text(4.0).replace(' ', ",")),
        )
        .expect("a writable folder");

        let built_in = Store::goal_named("Harman-Over-Ear-2018").expect("a built-in");
        let own = Store::goal_named(file.to_str().expect("UTF-8")).expect("a file");

        assert_eq!(
            built_in,
            BuiltIn::HarmanOverEar2018.response().expect("target")
        );
        assert_eq!(own.at(1_000.0), 4.0);
        assert!(matches!(
            Store::goal_named(folder.join("missing.csv").to_str().expect("UTF-8")),
            Err(Error::Store {
                op: StoreOp::Read,
                ..
            })
        ));
    }

    #[test]
    fn a_measurement_file_larger_than_a_measurement_may_be_is_refused() {
        let scratch = Scratch::new();
        let folder = scratch.store.folder().to_path_buf();
        fs::create_dir_all(&folder).expect("a writable folder");
        let file = folder.join("huge.txt");
        fs::write(&file, vec![b'1'; LARGEST_MEASUREMENT + 1]).expect("a writable folder");

        assert!(matches!(
            Store::response_in(&file),
            Err(Error::MeasurementTooLarge { .. })
        ));
    }
}
