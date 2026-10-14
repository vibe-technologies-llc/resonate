use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Write},
    os::fd::FromRawFd,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use parking_lot::Mutex;
use resonate_core::Mbid;
use resonate_providers::{
    Authorizing, Client, Delivery, Identity, Obtained, Opened, Provider, SignsIn,
};

use crate::{Error, MobilePlayer, Result, error::DownloadOp, features::SearchSong};

#[derive(Default)]
pub(crate) struct Downloads {
    offers: Mutex<BTreeMap<u64, Delivery>>,
    next: AtomicU64,
    bytes: AtomicU64,
    cancelled: AtomicBool,
    authorizing: Mutex<Option<(Client, Authorizing)>>,
    refreshed: Arc<Mutex<Option<String>>>,
}

#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum MusicProvider {
    Monochrome,
    Subsonic,
    Tidal,
    HifiApi,
}

#[derive(Clone, uniffi::Record)]
pub struct DownloadAccount {
    pub provider: MusicProvider,
    pub server: String,
    pub user: String,
    pub password: String,
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct DownloadOffer {
    pub id: u64,
    pub extension: String,
    pub name: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct TidalAuthorization {
    pub user_code: String,
    pub verify_at: String,
}

#[uniffi::export]
impl MobilePlayer {
    pub fn find_download(
        &self,
        song: SearchSong,
        account: DownloadAccount,
    ) -> Result<Option<DownloadOffer>> {
        self.downloads.cancelled.store(false, Ordering::Relaxed);
        self.downloads.bytes.store(0, Ordering::Relaxed);
        let DownloadAccount {
            provider,
            server,
            user,
            password,
            client_id,
            client_secret,
            refresh_token,
        } = account;
        let stem = format!("{} - {}", song.artist, song.title)
            .chars()
            .map(|letter| {
                if letter.is_control() || matches!(letter, '/' | '\\') {
                    '_'
                } else {
                    letter
                }
            })
            .collect::<String>();
        self.require_online()?;
        let identity = Identity {
            recording: Mbid::new(&song.recording).ok(),
            title: song.title,
            artist: Some(song.artist),
            album: (!song.album.is_empty()).then_some(song.album),
            length: song.duration_ms.map(Duration::from_millis),
            isrcs: song
                .isrcs
                .iter()
                .filter_map(|isrc| resonate_core::Isrc::new(isrc).ok())
                .collect(),
            ..Identity::default()
        };
        let provider: Box<dyn Provider> = match provider {
            MusicProvider::Monochrome => Box::new(if server.trim().is_empty() {
                resonate_monochrome::Monochrome::hosted()
            } else {
                resonate_monochrome::Monochrome::at(&server)
            }),
            MusicProvider::Subsonic => {
                Box::new(resonate_subsonic::Subsonic::at(resonate_subsonic::Server {
                    url: server,
                    user,
                    password,
                }))
            }
            MusicProvider::Tidal => {
                let refreshed = Arc::clone(&self.downloads.refreshed);
                Box::new(
                    resonate_tidal::Tidal::signed_in(resonate_tidal::Account {
                        client_id,
                        client_secret: (!client_secret.is_empty()).then_some(client_secret),
                        refresh_token,
                    })
                    .telling(move |token| {
                        *refreshed.lock() = Some(token.into_string());
                    }),
                )
            }
            MusicProvider::HifiApi => Box::new(resonate_tidal::HifiApi::hosted()),
        };
        let obtained = provider.find(&identity).map_err(|source| Error::Provider {
            op: DownloadOp::Find,
            source: Box::new(source),
        })?;
        if self.downloads.cancelled.load(Ordering::Relaxed) {
            return Err(Error::DownloadCancelled);
        }
        self.require_online()?;
        let Obtained::Found(delivery) = obtained else {
            return Ok(None);
        };
        let extension = match &delivery {
            Delivery::File(path) => path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("flac")
                .to_owned(),
            Delivery::Stream { extension, .. } => extension.as_str().to_owned(),
        };
        let id = self.downloads.next.fetch_add(1, Ordering::Relaxed);
        let mut offers = self.downloads.offers.lock();
        if offers.len() >= 4 {
            offers.pop_first();
        }
        offers.insert(id, delivery);
        let name = resonate_core::naming::named_within(
            "",
            std::ffi::OsStr::new(&stem),
            format!(".{extension}"),
        )
        .to_string_lossy()
        .into_owned();
        Ok(Some(DownloadOffer {
            id,
            extension,
            name,
        }))
    }

    pub fn download_offer(&self, id: u64, descriptor: i32) -> Result<u64> {
        let mut output = fresh_destination(descriptor)?;
        self.require_online()?;
        if self.downloads.cancelled.load(Ordering::Relaxed) {
            return Err(Error::DownloadCancelled);
        }
        self.downloads.bytes.store(0, Ordering::Relaxed);
        let delivery = self
            .downloads
            .offers
            .lock()
            .remove(&id)
            .ok_or(Error::DownloadExpired)?;
        let mut reader: Box<dyn Read + Send> = match delivery {
            Delivery::File(path) => {
                Box::new(File::open(path).map_err(|source| Error::DownloadIo {
                    op: DownloadOp::Open,
                    source,
                })?)
            }
            Delivery::Stream { opening, .. } => {
                match opening.open().map_err(|source| Error::Provider {
                    op: DownloadOp::Open,
                    source: Box::new(source),
                })? {
                    Opened::Reading(reader) => reader,
                    Opened::Gone => return Err(Error::DownloadExpired),
                }
            }
        };
        let mut bytes = 0_u64;
        let mut buffer = vec![0_u8; 64 * 1024];
        loop {
            if self.downloads.cancelled.load(Ordering::Relaxed)
                || !self.services.online.client().is_reaching()
            {
                return Err(Error::DownloadCancelled);
            }
            let count = reader
                .read(&mut buffer)
                .map_err(|source| Error::DownloadIo {
                    op: DownloadOp::Read,
                    source,
                })?;
            if count == 0 {
                break;
            }
            bytes = bytes.saturating_add(count as u64);
            if bytes > 8 * 1024 * 1024 * 1024 {
                return Err(Error::DownloadTooLarge);
            }
            output
                .write_all(&buffer[..count])
                .map_err(|source| Error::DownloadIo {
                    op: DownloadOp::Write,
                    source,
                })?;
            self.downloads.bytes.store(bytes, Ordering::Relaxed);
        }
        output.flush().map_err(|source| Error::DownloadIo {
            op: DownloadOp::Finish,
            source,
        })?;
        Ok(bytes)
    }

    pub fn authorize_tidal(&self, client_id: String, secret: String) -> Result<TidalAuthorization> {
        self.require_online()?;
        let client = Client {
            id: client_id,
            secret: (!secret.is_empty()).then_some(secret),
        };
        let authorizing = resonate_tidal::TidalSignIn::at(resonate_tidal::Endpoints::tidal())
            .authorizing(&client)
            .map_err(|source| Error::Provider {
                op: DownloadOp::Find,
                source: Box::new(source),
            })?;
        let request = TidalAuthorization {
            user_code: authorizing.user_code.clone(),
            verify_at: authorizing.verify_at.clone(),
        };
        *self.downloads.authorizing.lock() = Some((client, authorizing));
        Ok(request)
    }

    pub fn finish_tidal_authorization(&self) -> Result<Option<String>> {
        self.require_online()?;
        let (client, authorizing) = self
            .downloads
            .authorizing
            .lock()
            .take()
            .ok_or(Error::DownloadExpired)?;
        resonate_tidal::TidalSignIn::at(resonate_tidal::Endpoints::tidal())
            .authorized(&client, &authorizing, &|| {
                !self.services.online.client().is_reaching()
            })
            .map(|token| token.map(|token| token.into_string()))
            .map_err(|source| Error::Provider {
                op: DownloadOp::Find,
                source: Box::new(source),
            })
    }

    pub fn refreshed_tidal_token(&self) -> Option<String> {
        self.downloads.refreshed.lock().take()
    }

    pub fn download_bytes(&self) -> u64 {
        self.downloads.bytes.load(Ordering::Relaxed)
    }
    pub fn cancel_download(&self) {
        self.downloads.cancelled.store(true, Ordering::Relaxed);
    }

    pub fn validate_download(&self, uri: String, duration_ms: Option<u64>) -> Result<()> {
        let info = resonate_codec::probe(&self.sources, &crate::documents::location(&uri)?)?;
        if let Some(expected) = duration_ms
            && let Some(frames) = info.duration
            && frames
                .to_duration(info.spec.rate)
                .abs_diff(Duration::from_millis(expected))
                > Duration::from_secs(5)
        {
            return Err(Error::WrongDeliveryLength);
        }
        Ok(())
    }
}

pub(crate) fn fresh_destination(descriptor: i32) -> Result<File> {
    if descriptor < 0 {
        return Err(Error::InvalidDescriptor);
    }
    Ok(unsafe { File::from_raw_fd(descriptor) })
}
