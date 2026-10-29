use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

use parking_lot::Mutex;
use resonate_core::{SourceId, eq::Profile};
use resonate_eq::{Catalogue, Corrections, DeviceId, EqOp, LARGEST_PROFILE, read_profile};
use resonate_library::{Library, LookupOp};

use crate::{Client, Host, client::LARGEST_INDEX, query::escape_path};

const AUTOEQ: &str = "autoeq";
const INDEX_PATH: &str = "/results/INDEX.md";
const PARAMETRIC: &str = " ParametricEQ.txt";

pub(crate) const ASK_AGAIN_ABOUT_THE_INDEX: Duration = Duration::from_secs(30 * 24 * 60 * 60);
pub(crate) const ASK_AGAIN_ABOUT_A_DEVICE: Duration = Duration::from_secs(90 * 24 * 60 * 60);

fn still_fresh(taken: SystemTime, within: Duration) -> bool {
    SystemTime::now()
        .duration_since(taken)
        .is_ok_and(|age| age < within)
        || taken > SystemTime::now()
}

enum Remembered<T> {
    Fresh(T),
    Stale(T),
}

impl<T> Remembered<T> {
    fn aged(held: T, taken: SystemTime, within: Duration) -> Self {
        if still_fresh(taken, within) {
            Self::Fresh(held)
        } else {
            Self::Stale(held)
        }
    }
}

fn settled<T>(
    remembered: Option<Remembered<T>>,
    asked: impl FnOnce() -> resonate_eq::Result<T>,
) -> resonate_eq::Result<T> {
    match remembered {
        Some(Remembered::Fresh(kept)) => Ok(kept),
        Some(Remembered::Stale(kept)) => Ok(asked().unwrap_or_else(|error| {
            tracing::debug!(%error, "a correction could not be asked for again; reading what was kept");
            kept
        })),
        None => asked(),
    }
}

pub struct AutoEq {
    client: Arc<Client>,
    library: Option<Arc<Library>>,
    base: String,
    source: SourceId,
    read: Mutex<Option<Arc<Catalogue>>>,
}

impl AutoEq {
    pub fn new(client: Arc<Client>, library: Option<Arc<Library>>) -> Self {
        Self {
            client,
            library,
            base: Host::AutoEq.base().to_owned(),
            source: SourceId::new(AUTOEQ).unwrap_or_else(|_| SourceId::local()),
            read: Mutex::new(None),
        }
    }

    #[cfg(test)]
    fn answering_at(client: Arc<Client>, base: String) -> Self {
        Self {
            base,
            ..Self::new(client, None)
        }
    }

    fn refused(&self, error: crate::Error, op: EqOp) -> resonate_eq::Error {
        error.into_eq_error(self.source.clone(), op)
    }

    fn text_of(&self, op: LookupOp, path: &str, limit: usize) -> crate::Result<Option<String>> {
        let url = format!("{}{path}", self.base);
        let Some(bytes) = self.client.bytes(Host::AutoEq, op, &url, limit)? else {
            return Ok(None);
        };
        match String::from_utf8(bytes) {
            Ok(text) => Ok(Some(text)),
            Err(error) => {
                tracing::debug!(%error, url, "the answer was not text this build can read");
                Err(crate::Error::Unreadable {
                    host: Host::AutoEq,
                    op,
                })
            }
        }
    }

    fn remembered_index(&self) -> Option<Remembered<String>> {
        let library = self.library.as_ref()?;
        match library.kept_corrections_index() {
            Ok(Some(kept)) => Some(Remembered::aged(
                kept.text,
                kept.taken,
                ASK_AGAIN_ABOUT_THE_INDEX,
            )),
            Ok(None) => None,
            Err(error) => {
                tracing::debug!(%error, "the catalog could not say what index it kept");
                None
            }
        }
    }

    fn keep_index(&self, text: &str) {
        let Some(library) = self.library.as_ref() else {
            return;
        };
        if let Err(error) = library.keep_corrections_index(text) {
            tracing::debug!(%error, "the catalog could not keep the index");
        }
    }

    fn remembered_profile(&self, device: &DeviceId) -> Option<Remembered<Option<String>>> {
        let library = self.library.as_ref()?;
        match library.kept_correction(device.as_str()) {
            Ok(Some(kept)) => Some(Remembered::aged(
                kept.text,
                kept.taken,
                ASK_AGAIN_ABOUT_A_DEVICE,
            )),
            Ok(None) => None,
            Err(error) => {
                tracing::debug!(%error, device = device.as_str(), "the catalog could not say what it kept");
                None
            }
        }
    }

    fn keep_profile(&self, device: &DeviceId, text: Option<&str>) {
        let Some(library) = self.library.as_ref() else {
            return;
        };
        if let Err(error) = library.keep_correction(device.as_str(), text) {
            tracing::debug!(%error, device = device.as_str(), "the catalog could not keep the correction");
        }
    }

    fn fetch_index(&self) -> resonate_eq::Result<String> {
        let told = self
            .text_of(LookupOp::Devices, INDEX_PATH, LARGEST_INDEX)
            .map_err(|error| self.refused(error, EqOp::Index))?;

        let Some(text) = told.filter(|text| !Catalogue::read(text).devices().is_empty()) else {
            return Err(resonate_eq::Error::Unreadable {
                provider: self.source.clone(),
                op: EqOp::Index,
            });
        };
        self.keep_index(&text);
        Ok(text)
    }
}

impl AutoEq {
    fn fetch_profile(&self, device: &DeviceId) -> resonate_eq::Result<Option<String>> {
        let Some(path) = parametric_path(device) else {
            return Ok(None);
        };
        let told = self
            .text_of(LookupOp::Correction, &path, LARGEST_PROFILE)
            .map_err(|error| self.refused(error, EqOp::Fetch))?;
        if told
            .as_deref()
            .is_some_and(|text| !reads_as_a_correction(text))
        {
            return Err(resonate_eq::Error::Unreadable {
                provider: self.source.clone(),
                op: EqOp::Fetch,
            });
        }

        self.keep_profile(device, told.as_deref());
        Ok(told)
    }
}

fn reads_as_a_correction(text: &str) -> bool {
    read_profile(text).is_ok_and(|profile| !profile.is_transparent())
}

pub(crate) fn parametric_path(device: &DeviceId) -> Option<String> {
    let label = device.as_str().rsplit('/').next()?;
    Some(format!(
        "/results/{}{}",
        escape_path(device.as_str()),
        escape_path(&format!("/{label}{PARAMETRIC}"))
    ))
}

impl Corrections for AutoEq {
    fn source(&self) -> &SourceId {
        &self.source
    }

    fn catalogue(&self) -> resonate_eq::Result<Arc<Catalogue>> {
        if let Some(read) = self.read.lock().clone() {
            return Ok(read);
        }

        let text = settled(self.remembered_index(), || self.fetch_index())?;

        let catalogue = Arc::new(Catalogue::read(&text));
        *self.read.lock() = Some(Arc::clone(&catalogue));
        Ok(catalogue)
    }

    fn profile(&self, device: &DeviceId) -> resonate_eq::Result<Option<Profile>> {
        let told = settled(self.remembered_profile(device), || {
            self.fetch_profile(device)
        })?;
        told.as_deref().map(read_profile).transpose()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread::{self, JoinHandle},
    };

    use resonate_eq::{Corrected, search, suggest};

    use super::*;
    use crate::{
        Identity,
        client::{Carried, Introduction, WallClock},
    };

    const INDEX: &str = include_str!("../tests/fixtures/autoeq_index.md");
    const PARAMETRIC_TEXT: &str = include_str!("../tests/fixtures/autoeq_parametric.txt");

    #[test]
    fn a_page_that_is_no_correction_is_not_read_as_one() {
        assert!(reads_as_a_correction(PARAMETRIC_TEXT));
        assert!(!reads_as_a_correction(
            "<html><body>Sign in to the network to continue</body></html>"
        ));
        assert!(
            Catalogue::read("<html><body>Sign in</body></html>")
                .devices()
                .is_empty()
        );
        assert!(!Catalogue::read(INDEX).devices().is_empty());
    }

    #[test]
    fn a_captured_index_maps_to_the_devices_it_names() {
        let catalogue = Catalogue::read(INDEX);

        assert!(catalogue.len() >= 3);
        let hd650 = catalogue
            .devices()
            .iter()
            .find(|device| device.label == "Sennheiser HD 650")
            .expect("the index names it");
        assert_eq!(hd650.measured_by, "oratory1990");
    }

    #[test]
    fn a_captured_profile_maps_to_the_bands_it_names() {
        let profile = read_profile(PARAMETRIC_TEXT).expect("a well formed profile");

        assert_eq!(profile.bands().len(), 10);
        assert_eq!(profile.preamp().milli_decibels(), -6_100);
    }

    #[test]
    fn a_device_names_the_file_its_measurement_is_written_in() {
        let device = DeviceId::new("oratory1990/over-ear/Sennheiser HD 650").expect("a path");

        assert_eq!(
            parametric_path(&device).as_deref(),
            Some(
                "/results/oratory1990/over-ear/Sennheiser%20HD%20650/Sennheiser%20HD%20650%20ParametricEQ.txt"
            )
        );
    }

    #[test]
    fn a_device_whose_name_carries_syntax_is_escaped_once_and_keeps_its_path() {
        let device = DeviceId::new(
            "crinacle/Bruel & Kjaer 4620 in-ear/Moondrop x Crinacle DUSK (Harman DSP)",
        )
        .expect("a path");
        let path = parametric_path(&device).expect("a path");

        assert!(
            path.starts_with("/results/crinacle/Bruel%20%26%20Kjaer"),
            "{path}"
        );
        assert!(
            path.ends_with("%28Harman%20DSP%29%20ParametricEQ.txt"),
            "{path}"
        );
        assert_eq!(path.matches("/results/").count(), 1);
    }

    #[test]
    fn the_captured_index_answers_the_way_the_whole_one_does() {
        let catalogue = Catalogue::read(INDEX);
        let named = |description: &str| {
            suggest(&catalogue, description)
                .and_then(|found| catalogue.device(found))
                .map(|device| device.label.clone())
        };

        assert_eq!(
            named("Sennheiser HD 650").as_deref(),
            Some("Sennheiser HD 650")
        );
        assert_eq!(named("USB-C to 3.5mm Headphone Jack Adapter"), None);
        assert!(!search(&catalogue, "hd 650").is_empty());
    }

    #[test]
    fn a_reading_that_has_not_gone_stale_is_kept_rather_than_asked_for_again() {
        assert!(still_fresh(SystemTime::now(), ASK_AGAIN_ABOUT_THE_INDEX));
        assert!(!still_fresh(
            SystemTime::now() - ASK_AGAIN_ABOUT_THE_INDEX - Duration::from_secs(1),
            ASK_AGAIN_ABOUT_THE_INDEX
        ));
        assert!(still_fresh(
            SystemTime::now() - Duration::from_secs(40 * 24 * 60 * 60),
            ASK_AGAIN_ABOUT_A_DEVICE
        ));
    }

    fn offline() -> resonate_eq::Result<&'static str> {
        Err(resonate_eq::Error::Unreadable {
            provider: SourceId::local(),
            op: EqOp::Index,
        })
    }

    #[test]
    fn what_was_kept_is_read_while_fresh_and_when_asking_again_fails() {
        assert_eq!(
            settled(Some(Remembered::Fresh("kept")), || panic!(
                "a fresh answer was asked for again"
            ))
            .ok(),
            Some("kept")
        );
        assert_eq!(
            settled(Some(Remembered::Stale("kept")), offline).ok(),
            Some("kept"),
            "an offline session lost a stale answer it still held"
        );
        assert_eq!(
            settled(Some(Remembered::Stale("kept")), || Ok("asked")).ok(),
            Some("asked")
        );
        assert!(settled(None, offline).is_err());
    }

    struct Asked {
        path: String,
        user_agent: String,
    }

    fn serving(
        answers: Vec<(String, Option<&'static str>)>,
        requests: usize,
    ) -> (String, JoinHandle<Vec<Asked>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let base = format!("http://{}", listener.local_addr().expect("an address"));
        let served = thread::spawn(move || {
            let mut asked = Vec::new();
            for _ in 0..requests {
                let (mut stream, _) = listener.accept().expect("a connection");
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n")
                    && stream.read(&mut byte).expect("a request") == 1
                {
                    head.push(byte[0]);
                }
                let head = String::from_utf8(head).expect("a head in ASCII");
                let path = head
                    .lines()
                    .next()
                    .and_then(|line| line.split(' ').nth(1))
                    .expect("a request line")
                    .to_owned();
                let user_agent = head
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("user-agent")
                            .then(|| value.trim().to_owned())
                    })
                    .unwrap_or_default();

                let body = answers
                    .iter()
                    .find(|(wanted, _)| *wanted == path)
                    .and_then(|(_, body)| *body);
                let response = match body {
                    Some(body) => format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    ),
                    None => {
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            .to_owned()
                    }
                };
                let _ = stream.write_all(response.as_bytes());
                asked.push(Asked { path, user_agent });
            }
            asked
        });
        (base, served)
    }

    fn over_loopback(base: String) -> AutoEq {
        let client = Client::on_clock(
            Introduction::as_(&Identity::of_this_build()),
            Arc::new(WallClock),
            Carried::Plain,
        );
        AutoEq::answering_at(Arc::new(client), base)
    }

    #[test]
    fn the_whole_index_is_asked_for_once_and_lists_every_measurer_of_a_model_beside_the_others() {
        const INDEX_PATH: &str = "/results/INDEX.md";
        let ananda_by_crinacle =
            DeviceId::new("crinacle/GRAS 43AG-7 over-ear/HIFIMAN Ananda").expect("a path");
        let ananda_by_oratory =
            DeviceId::new("oratory1990/over-ear/HIFIMAN Ananda").expect("a path");
        let by_crinacle = parametric_path(&ananda_by_crinacle).expect("a path");
        let (base, served) = serving(
            vec![
                (INDEX_PATH.to_owned(), Some(INDEX)),
                (by_crinacle, Some(PARAMETRIC_TEXT)),
            ],
            3,
        );
        let source = over_loopback(base);

        let first = source.catalogue().expect("the index is read");
        let second = source.catalogue().expect("the index is kept for the run");
        let sought = search(&first, "ananda");
        let measurers: Vec<&str> = sought
            .iter()
            .filter_map(|found| first.device(*found))
            .map(|device| device.measured_by.as_str())
            .collect();
        let fetched = Corrected::uncorrected()
            .and(Arc::new(source))
            .profiles(&[ananda_by_crinacle, ananda_by_oratory]);
        let asked = served.join().expect("the server");

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(measurers, vec!["oratory1990", "crinacle"]);
        assert_eq!(
            asked
                .iter()
                .filter(|asked| asked.path == INDEX_PATH)
                .count(),
            1
        );
        assert_eq!(asked.len(), 3);
        assert!(matches!(&fetched[0].profile, Ok(Some(profile)) if profile.bands().len() == 10));
        assert!(matches!(fetched[1].profile, Ok(None)));
        let named = format!("resonate/{}", env!("CARGO_PKG_VERSION"));
        assert!(
            asked.iter().all(|asked| asked.user_agent == named),
            "a request said more than what this build is: {:?}",
            asked
                .iter()
                .map(|asked| &asked.user_agent)
                .collect::<Vec<_>>()
        );
    }
}
