use resonate_sync::SyncKey;

use super::{Reaching, Running, SyncConfig, SyncStatus};
use crate::{Error, MobilePlayer, Result, error::SyncOp};

#[derive(Clone)]
pub(super) struct Failed {
    pub(super) config: SyncConfig,
    pub(super) key: SyncKey,
}

#[uniffi::export]
pub fn sync_new_code() -> Result<String> {
    SyncKey::minted()
        .map(|key| key.code())
        .map_err(|source| Error::sync(SyncOp::Mint, source))
}

#[uniffi::export]
pub fn sync_reads_as_a_code(code: String) -> bool {
    SyncKey::from_code(&code).is_ok()
}

#[uniffi::export]
pub fn sync_code_spelled(code: String) -> Option<String> {
    SyncKey::from_code(code.trim()).ok().map(|key| key.code())
}

fn key_of(config: &SyncConfig) -> Result<SyncKey> {
    if config.key.trim().is_empty() {
        return Err(Error::NoPairingCode);
    }
    SyncKey::from_code(config.key.trim()).map_err(|source| Error::sync(SyncOp::ReadCode, source))
}

#[uniffi::export]
impl MobilePlayer {
    pub fn sync_start(&self, config: SyncConfig) -> Result<SyncStatus> {
        let key = config.enabled.then(|| key_of(&config)).transpose()?;

        if !config.musicbrainz_copy {
            self.syncing.copied.forget_removal();
        }

        *self.syncing.reaching.lock() = Reaching::of(&config, &self.syncing.folder);

        let mut running = self.syncing.running.lock();

        *self.syncing.failed.lock() = None;
        if let Some(previous) = running.take() {
            previous.sync.leave();
        }

        let keeps_a_copy = config.enabled && config.musicbrainz_copy;

        if let Some(key) = key {
            if !config.nearby_only {
                self.require_online()?;
            }
            *running = Some(self.started_or_failed(config, key)?);
        }
        drop(running);
        self.syncing.copied.keep(keeps_a_copy, self.copy_reaching());
        Ok(self.sync_status())
    }

    pub fn sync_retry(&self) -> Result<SyncStatus> {
        let mut running = self.syncing.running.lock();
        let failed = self.syncing.failed.lock().clone();

        if let Some(Failed { config, key }) = failed.filter(|_| running.is_none()) {
            let keeps_a_copy = config.musicbrainz_copy;

            if !config.nearby_only {
                self.require_online()?;
            }
            *self.syncing.reaching.lock() = Reaching::of(&config, &self.syncing.folder);
            *running = Some(self.started_or_failed(config, key)?);
            *self.syncing.failed.lock() = None;
            drop(running);
            self.syncing.copied.keep(keeps_a_copy, self.copy_reaching());
        } else {
            drop(running);
        }
        Ok(self.sync_status())
    }
}

impl MobilePlayer {
    fn started_or_failed(&self, config: SyncConfig, key: SyncKey) -> Result<Running> {
        let failed = Failed {
            config: config.clone(),
            key: key.clone(),
        };

        self.started(config, key)
            .inspect_err(|_| *self.syncing.failed.lock() = Some(failed))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::sync::{
        SyncStanding,
        driving::SyncControl,
        tests::{folder, player},
    };

    fn nearby(key: &str) -> SyncConfig {
        SyncConfig {
            enabled: true,
            nearby_only: true,
            server: String::new(),
            key: key.to_owned(),
            device: "Phone".to_owned(),
            favourites: false,
            playlists: false,
            everything: false,
            metered_allowed: true,
            control: SyncControl::Nothing,
            stun: String::new(),
            certificate: String::new(),
            musicbrainz_copy: false,
            takes_deletions: true,
        }
    }

    fn this_device(player: &MobilePlayer) -> Option<String> {
        player
            .syncing
            .running
            .lock()
            .as_ref()
            .map(|running| running.sync.device().to_string())
    }

    #[test]
    fn a_pairing_code_is_minted_and_read_back_in_its_own_spelling() {
        let code = sync_new_code().expect("a pairing code");

        assert!(sync_reads_as_a_code(code.clone()));
        assert!(!sync_reads_as_a_code("not a code".into()));
        assert_eq!(
            sync_code_spelled(format!(" {} ", code.to_lowercase())),
            Some(code)
        );
        assert_eq!(sync_code_spelled("not a code".into()), None);
    }

    #[test]
    fn a_code_that_reads_as_none_is_refused_before_the_group_running_is_left_and_the_kept_one_joins_it_again()
     {
        let folder = folder("pairing-refused");
        let kept = sync_new_code().expect("a pairing code");
        let player = player(&folder);

        player.sync_start(nearby(&kept)).expect("sync started");

        let started = this_device(&player);
        let refused = player.sync_start(nearby("ABCDE-FGHJK-not-a-code"));
        let still = player.sync_status();

        assert!(matches!(
            refused,
            Err(Error::Sync {
                op: SyncOp::ReadCode,
                ..
            })
        ));
        assert_eq!(still.code, kept);
        assert_eq!(still.standing, SyncStanding::Nearby);
        assert!(player.sync_nearby_record().is_some());

        player.sync_stop();
        drop(player);

        let reopened = self::player(&folder);
        let again = reopened
            .sync_start(nearby(&kept))
            .expect("sync started again with the kept code");
        let joined_as = this_device(&reopened);

        reopened.sync_stop();

        let _ = fs::remove_dir_all(&folder);

        assert_eq!(again.code, kept);
        assert!(started.is_some());
        assert_eq!(joined_as, started);
    }

    #[test]
    fn a_blank_key_is_refused_so_no_code_is_offered_that_was_not_kept_first() {
        let folder = folder("pairing-blank");
        let player = player(&folder);

        let refused = player.sync_start(nearby(" "));
        let off = player.sync_status();

        assert!(matches!(refused, Err(Error::NoPairingCode)));
        assert_eq!(off.standing, SyncStanding::Off);
        assert!(off.code.is_empty());
        assert!(player.sync_nearby_record().is_none());

        let kept = sync_new_code().expect("a pairing code");

        player.sync_start(nearby(&kept)).expect("sync started");

        let refused_while_running = player.sync_start(nearby(""));
        let running = player.sync_status();

        player.sync_stop();
        drop(player);

        let reopened = self::player(&folder);
        let again = reopened
            .sync_start(nearby(&kept))
            .expect("sync started again with the kept code");

        reopened.sync_stop();

        let _ = fs::remove_dir_all(&folder);

        assert!(matches!(refused_while_running, Err(Error::NoPairingCode)));
        assert_eq!(running.code, kept);
        assert_eq!(running.standing, SyncStanding::Nearby);
        assert_eq!(again.code, kept);
    }

    #[test]
    fn sync_switched_off_needs_no_code_and_a_retry_with_nothing_failed_starts_nothing() {
        let folder = folder("pairing-off");
        let player = player(&folder);

        let off = player
            .sync_start(SyncConfig {
                enabled: false,
                ..nearby("")
            })
            .expect("sync left off");
        let retried = player.sync_retry().expect("nothing to retry");

        let _ = fs::remove_dir_all(&folder);

        assert_eq!(off.standing, SyncStanding::Off);
        assert_eq!(retried.standing, SyncStanding::Off);
        assert!(retried.code.is_empty());
    }

    #[test]
    fn a_start_that_fails_is_told_as_failed_and_a_retry_takes_it_up_once_the_fault_clears() {
        let folder = folder("pairing-failed");
        let kept = sync_new_code().expect("a pairing code");
        let player = player(&folder);
        let ledger = folder.join(super::super::LEDGER);

        fs::create_dir_all(ledger.join("in-the-way")).expect("a folder where the ledger goes");

        let failed = player.sync_start(nearby(&kept));
        let told = player.sync_status();
        let still_failed = player.sync_retry();
        let told_again = player.sync_status();

        fs::remove_dir_all(&ledger).expect("the folder in the way taken away");

        let retried = player.sync_retry().expect("the retry started sync");
        let running = player.sync_status();

        player.sync_stop();

        let stopped = player.sync_status();

        let _ = fs::remove_dir_all(&folder);

        assert!(matches!(
            failed,
            Err(Error::Sync {
                op: SyncOp::Start,
                ..
            })
        ));
        assert_eq!(told.standing, SyncStanding::Failed);
        assert_eq!(told.code, kept);
        assert!(still_failed.is_err());
        assert_eq!(told_again.standing, SyncStanding::Failed);
        assert_eq!(retried.standing, SyncStanding::Nearby);
        assert_eq!(running.code, kept);
        assert_eq!(stopped.standing, SyncStanding::Off);
        assert!(stopped.code.is_empty());
    }
}
