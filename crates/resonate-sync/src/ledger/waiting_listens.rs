use std::{collections::BTreeSet, time::Duration};

use rusqlite::params;

use super::{Ledger, device_from};
use crate::{
    Error, Result, StoreOp,
    catalog::{Catalog, RemoteListen},
    wire::decoded_recording,
};

struct Uncredited {
    device: Vec<u8>,
    id: i64,
    recording: Vec<u8>,
    at: i64,
    heard: i64,
}

impl Uncredited {
    fn remote(&self) -> Result<RemoteListen> {
        Ok(RemoteListen {
            from: device_from(&self.device),
            id: self.id.cast_unsigned(),
            recording: decoded_recording(&self.recording)?,
            at: self.at,
            heard: Duration::from_millis(self.heard.cast_unsigned()),
        })
    }
}

impl Ledger {
    pub(super) fn settle_listens(&mut self, catalog: &dyn Catalog) -> Result<usize> {
        let revision = catalog.revision();

        if self.listens_tried_at == Some(revision) {
            return Ok(0);
        }

        let mut absent = BTreeSet::new();
        let mut settled = 0;

        for waiting in self.uncredited_listens()? {
            if absent.contains(&waiting.recording) {
                continue;
            }
            if catalog.credit(&waiting.remote()?).map_err(Error::Catalog)? {
                self.mark_credited(&waiting.device, waiting.id)?;
                settled += 1;
            } else {
                absent.insert(waiting.recording);
            }
        }
        self.listens_tried_at = Some(revision);
        Ok(settled)
    }

    pub(super) fn credit_arrived(
        &mut self,
        catalog: &dyn Catalog,
        listen: &RemoteListen,
    ) -> Result<bool> {
        let credited = catalog.credit(listen).map_err(|refused| {
            self.listens_tried_at = None;
            Error::Catalog(refused)
        })?;

        if credited {
            self.mark_credited(listen.from.0.as_slice(), listen.id.cast_signed())?;
        }
        Ok(credited)
    }

    fn mark_credited(&self, device: &[u8], id: i64) -> Result<()> {
        self.connection
            .execute(
                "UPDATE listens SET credited = 1 WHERE device = ?1 AND id = ?2",
                params![device, id],
            )
            .map_err(|source| Error::store(StoreOp::Write, source))?;
        Ok(())
    }

    fn uncredited_listens(&self) -> Result<Vec<Uncredited>> {
        self.connection
            .prepare_cached(
                "SELECT device, id, recording, at, heard FROM listens WHERE credited = 0",
            )
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| {
                        Ok(Uncredited {
                            device: row.get(0)?,
                            id: row.get(1)?,
                            recording: row.get(2)?,
                            at: row.get(3)?,
                            heard: row.get(4)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(|source| Error::store(StoreOp::Read, source))
    }
}
