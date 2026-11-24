use std::{
    fs,
    io::{self, Write as _},
    path::Path,
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use resonate_core::{
    file_facts::sync_directory,
    writer::{self, Held, Writer},
};
use resonate_sync::check_trusted_certificate;

use super::{CERTIFICATE, STAGED_CERTIFICATE};
use crate::{Error, Result, StoreOp};

static STAGED: AtomicU64 = AtomicU64::new(0);

pub(super) fn keep(folder: &Path, pem: &[u8]) -> Result<()> {
    sweep(folder);

    let staged = folder.join(format!(
        "{}-{}.{STAGED_CERTIFICATE}",
        process::id(),
        STAGED.fetch_add(1, Ordering::Relaxed)
    ));
    let held = Held::made(&staged).map_err(written)?;

    let installed = stage(&held, &staged, pem).and_then(|()| install(folder, &staged));

    if installed.is_err() {
        let _ = fs::remove_file(&staged);
    }
    drop(held);
    installed
}

fn stage(held: &Held, staged: &Path, pem: &[u8]) -> Result<()> {
    let mut file = held.file();

    file.write_all(pem)
        .and_then(|()| file.sync_all())
        .map_err(written)?;

    check_trusted_certificate(staged).map_err(|source| Error::SyncCertificate {
        source: Box::new(source),
    })
}

fn install(folder: &Path, staged: &Path) -> Result<()> {
    fs::rename(staged, folder.join(CERTIFICATE))
        .and_then(|()| sync_directory(folder))
        .map_err(written)
}

fn sweep(folder: &Path) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };

    for entry in entries.filter_map(io::Result::ok) {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        let gone = if name == STAGED_CERTIFICATE {
            true
        } else {
            stamp_of(name).is_some_and(|stamp| writer::writer_of(&path, stamp) == Writer::Gone)
        };

        if gone {
            let _ = fs::remove_file(&path);
        }
    }
}

fn stamp_of(name: &str) -> Option<u32> {
    name.ends_with(STAGED_CERTIFICATE)
        .then(|| name.split_once('-'))
        .flatten()?
        .0
        .parse()
        .ok()
}

pub(super) fn forget(folder: &Path) -> Result<()> {
    match fs::remove_file(folder.join(CERTIFICATE)) {
        Err(source) if source.kind() != std::io::ErrorKind::NotFound => Err(written(source)),
        _ => Ok(()),
    }
}

fn written(source: std::io::Error) -> Error {
    Error::SyncCertificateFile {
        op: StoreOp::Write,
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, thread};

    use resonate_sync::SyncKey;

    use super::*;
    use crate::{
        harness::Harness,
        sync::{SyncConfig, SyncStanding, driving::SyncControl},
    };

    const CERTIFICATE: &str = "-----BEGIN CERTIFICATE-----
MIIBPzCB8qADAgECAhQ2UC2u+5Ud0LZcMNd3IY9deEf6OzAFBgMrZXAwGDEWMBQG
A1UEAwwNcmVzb25hdGUudGVzdDAgFw0yNjExMTIwMDAzMjdaGA8yMTI2MTAxOTAw
MDMyN1owGDEWMBQGA1UEAwwNcmVzb25hdGUudGVzdDAqMAUGAytlcAMhAAyUqYvN
3rvd0dg8J3gtCUtJyBYYd5xJNxqEE0YZxwUMo0wwSjAdBgNVHQ4EFgQUb9E2UNGs
KXN3jTCYsyQRXfJo9KowDwYDVR0TAQH/BAUwAwEB/zAYBgNVHREEETAPgg1yZXNv
bmF0ZS50ZXN0MAUGAytlcANBAFZ/s8JwQNWvIr2EU4Qr0GS2S2fA9XPGO5Tr7moO
uRACi1+SkHU377h5XBXccA0ghqVn5cUhYbvxj5+8g3VD/Ak=
-----END CERTIFICATE-----
";

    fn staged_in(folder: &Path) -> Vec<String> {
        fs::read_dir(folder)
            .expect("the folder")
            .filter_map(io::Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| name.ends_with(STAGED_CERTIFICATE))
            .collect()
    }

    fn nearby(certificate: &str, stun: &str) -> SyncConfig {
        SyncConfig {
            enabled: true,
            nearby_only: true,
            server: String::new(),
            key: SyncKey::minted().expect("a key").code().to_owned(),
            device: "Phone".to_owned(),
            favourites: false,
            playlists: false,
            everything: false,
            metered_allowed: true,
            control: SyncControl::Everything,
            stun: stun.to_owned(),
            certificate: certificate.to_owned(),
            musicbrainz_copy: false,
            takes_deletions: true,
        }
    }

    #[test]
    fn a_certificate_kept_is_trusted_by_sync_and_by_parties_and_one_forgotten_is_not() {
        let harness = Harness::new();
        let player = &harness.player;

        player.configure_online(true);
        player
            .sync_keep_certificate(CERTIFICATE.as_bytes().to_vec())
            .expect("a certificate kept");
        player
            .sync_start(nearby("ca.pem", " stun.example.org:3479 "))
            .expect("sync started");

        let reaching = player.syncing.reaching.lock().clone();

        assert_eq!(
            reaching.trusted.as_deref(),
            Some(harness.folder.join(super::CERTIFICATE).as_path())
        );
        assert_eq!(reaching.stun.as_deref(), Some("stun.example.org:3479"));
        assert_eq!(
            player.party_settings(None).expect("settings").trusted,
            reaching.trusted
        );

        player.sync_stop();
        player.sync_forget_certificate().expect("forgotten");
        player
            .sync_start(nearby("", ""))
            .expect("sync started without it");

        let reaching = player.syncing.reaching.lock().clone();

        assert!(reaching.trusted.is_none());
        assert!(reaching.stun.is_none());
        assert!(!harness.folder.join(super::CERTIFICATE).exists());
    }

    #[test]
    fn a_file_that_is_no_certificate_is_refused_and_leaves_the_kept_one_alone() {
        let harness = Harness::new();
        let player = &harness.player;

        player
            .sync_keep_certificate(CERTIFICATE.as_bytes().to_vec())
            .expect("a certificate kept");

        let refused = player.sync_keep_certificate(b"not a certificate".to_vec());

        assert!(matches!(refused, Err(Error::SyncCertificate { .. })));
        assert_eq!(
            fs::read_to_string(harness.folder.join(super::CERTIFICATE)).expect("the kept one"),
            CERTIFICATE
        );
        assert!(staged_in(&harness.folder).is_empty());
        assert!(player.sync_forget_certificate().is_ok());
        assert!(player.sync_forget_certificate().is_ok());
    }

    #[test]
    fn racing_imports_and_refused_ones_leave_one_complete_certificate_installed() {
        let harness = Harness::new();
        let player = &harness.player;
        let doubled = format!("{CERTIFICATE}{CERTIFICATE}");

        let outcomes = thread::scope(|scope| {
            (0..12)
                .map(|import| {
                    let pem = match import % 3 {
                        0 => CERTIFICATE.as_bytes().to_vec(),
                        1 => doubled.as_bytes().to_vec(),
                        _ => b"not a certificate".to_vec(),
                    };
                    scope.spawn(move || player.sync_keep_certificate(pem))
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|import| import.join().expect("an import"))
                .collect::<Vec<_>>()
        });

        let kept = fs::read_to_string(harness.folder.join(super::CERTIFICATE)).expect("the kept");

        assert!(kept == CERTIFICATE || kept == doubled);
        assert!(check_trusted_certificate(&harness.folder.join(super::CERTIFICATE)).is_ok());
        assert!(staged_in(&harness.folder).is_empty());
        assert_eq!(
            outcomes
                .iter()
                .filter(|outcome| matches!(outcome, Err(Error::SyncCertificate { .. })))
                .count(),
            4
        );
        assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 8);
    }

    #[test]
    fn a_stage_that_cannot_be_made_fails_and_leaves_the_kept_certificate_alone() {
        let harness = Harness::new();
        let player = &harness.player;

        player
            .sync_keep_certificate(CERTIFICATE.as_bytes().to_vec())
            .expect("a certificate kept");

        let folder = harness.folder.join(super::CERTIFICATE);
        let refused = keep(&folder, CERTIFICATE.as_bytes());

        assert!(matches!(refused, Err(Error::SyncCertificateFile { .. })));
        assert_eq!(
            fs::read_to_string(&folder).expect("the kept one"),
            CERTIFICATE
        );
    }

    #[test]
    fn a_stage_a_dead_writer_left_is_swept_and_a_living_one_is_left_alone() {
        let harness = Harness::new();
        let player = &harness.player;
        let dead = harness
            .folder
            .join(format!("{}-0.{STAGED_CERTIFICATE}", u32::MAX - 1));
        let legacy = harness.folder.join(STAGED_CERTIFICATE);
        let living = harness
            .folder
            .join(format!("{}-7.{STAGED_CERTIFICATE}", process::id()));

        fs::write(&dead, b"half").expect("a dead stage");
        fs::write(&legacy, b"half").expect("a legacy stage");

        let held = Held::made(&living).expect("a living stage");

        player
            .sync_keep_certificate(CERTIFICATE.as_bytes().to_vec())
            .expect("a certificate kept");

        assert!(!dead.exists());
        assert!(!legacy.exists());
        assert!(living.exists());

        drop(held);
    }

    #[test]
    fn a_certificate_named_but_gone_stops_sync_from_starting() {
        let harness = Harness::new();

        let started = harness.player.sync_start(nearby("ca.pem", ""));

        assert!(matches!(started, Err(Error::Sync { .. })));
    }

    #[test]
    fn a_restart_whose_certificate_is_gone_is_told_as_failed_and_a_retry_takes_it_up_once_it_is_kept()
     {
        let harness = Harness::new();
        let player = &harness.player;
        let running = nearby("", "");
        let replaced = SyncConfig {
            certificate: "ca.pem".to_owned(),
            ..running.clone()
        };

        let first = player.sync_start(running).expect("sync started");
        let restarted = player.sync_start(replaced);
        let failed = player.sync_status();

        player
            .sync_keep_certificate(CERTIFICATE.as_bytes().to_vec())
            .expect("a certificate kept");

        let retried = player.sync_retry().expect("the retry started sync");
        let reaching = player.syncing.reaching.lock().clone();

        player.sync_stop();

        assert!(matches!(restarted, Err(Error::Sync { .. })));
        assert_eq!(failed.standing, SyncStanding::Failed);
        assert_eq!(failed.code, first.code);
        assert!(failed.devices.is_empty());
        assert_eq!(retried.standing, SyncStanding::Nearby);
        assert_eq!(retried.code, first.code);
        assert_eq!(
            reaching.trusted.as_deref(),
            Some(harness.folder.join(super::CERTIFICATE).as_path())
        );
    }
}
