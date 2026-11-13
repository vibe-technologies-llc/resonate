use std::{fs, path::Path};

use resonate_sync::check_trusted_certificate;

use super::{CERTIFICATE, STAGED_CERTIFICATE};
use crate::{Error, Result, StoreOp};

pub(super) fn keep(folder: &Path, pem: &[u8]) -> Result<()> {
    let staged = folder.join(STAGED_CERTIFICATE);

    fs::write(&staged, pem).map_err(written)?;

    let checked = check_trusted_certificate(&staged).map_err(|source| Error::SyncCertificate {
        source: Box::new(source),
    });
    if let Err(refusal) = checked {
        let _ = fs::remove_file(&staged);
        return Err(refusal);
    }

    fs::rename(&staged, folder.join(CERTIFICATE)).map_err(written)
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
    use std::fs;

    use resonate_sync::SyncKey;

    use super::*;
    use crate::{
        harness::Harness,
        sync::{SyncConfig, driving::SyncControl},
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
        assert!(!harness.folder.join(super::STAGED_CERTIFICATE).exists());
        assert!(player.sync_forget_certificate().is_ok());
        assert!(player.sync_forget_certificate().is_ok());
    }

    #[test]
    fn a_certificate_named_but_gone_stops_sync_from_starting() {
        let harness = Harness::new();

        let started = harness.player.sync_start(nearby("ca.pem", ""));

        assert!(matches!(started, Err(Error::Sync { .. })));
    }
}
