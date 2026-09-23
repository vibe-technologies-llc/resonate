use std::collections::HashMap;

use zbus::{
    blocking::{Connection, Proxy},
    zvariant::{OwnedObjectPath, Value},
};

use crate::{BusOp, Error, Host, Result};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";
const INHIBIT: &str = "org.freedesktop.portal.Inhibit";
const REQUEST: &str = "org.freedesktop.portal.Request";
const NO_WINDOW: &str = "";
const SUSPEND: u32 = 4;
const IDLE: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Grip {
    Take,
    LetGo,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Gripping {
    held: bool,
}

impl Gripping {
    pub(crate) const fn toward(&mut self, wanted: bool) -> Option<Grip> {
        match (self.held, wanted) {
            (false, true) => {
                self.held = true;
                Some(Grip::Take)
            }
            (true, false) => {
                self.held = false;
                Some(Grip::LetGo)
            }
            (false, false) | (true, true) => None,
        }
    }
}

pub(crate) struct Awake {
    proxy: Proxy<'static>,
    reason: String,
    request: Option<OwnedObjectPath>,
}

impl Awake {
    pub(crate) fn start(connection: &Connection, host: &dyn Host) -> Option<Self> {
        match Self::new(connection, host) {
            Ok(awake) => Some(awake),
            Err(error) => {
                tracing::warn!(%error, "the session may blank or suspend while a track plays");
                None
            }
        }
    }

    fn new(connection: &Connection, host: &dyn Host) -> Result<Self> {
        let proxy = Proxy::new(connection, PORTAL, OBJECT_PATH, INHIBIT)
            .map_err(|source| Error::bus(BusOp::Connect, source))?;

        Ok(Self {
            proxy,
            reason: format!("{} is playing", host.identity()),
            request: None,
        })
    }

    pub(crate) fn hold(&mut self) {
        if self.request.is_some() {
            return;
        }
        let options: HashMap<&str, Value<'_>> =
            HashMap::from([("reason", Value::from(self.reason.as_str()))]);
        let asked = self
            .proxy
            .call::<_, _, OwnedObjectPath>("Inhibit", &(NO_WINDOW, IDLE | SUSPEND, options));

        match asked {
            Ok(request) => self.request = Some(request),
            Err(source) => {
                let error = Error::bus(BusOp::Inhibit, source);
                tracing::debug!(%error, "the session was not asked to stay awake");
            }
        }
    }

    pub(crate) fn release(&mut self) {
        let Some(request) = self.request.take() else {
            return;
        };
        let closed = Proxy::new(
            self.proxy.connection(),
            PORTAL,
            request.into_inner(),
            REQUEST,
        )
        .map_err(|source| Error::bus(BusOp::Connect, source))
        .and_then(|request| {
            request
                .call::<_, _, ()>("Close", &())
                .map_err(|source| Error::bus(BusOp::Release, source))
        });

        if let Err(error) = closed {
            tracing::debug!(%error, "the session was left held awake");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_session_is_taken_hold_of_once_and_let_go_of_once() {
        let mut gripping = Gripping::default();

        assert_eq!(gripping.toward(true), Some(Grip::Take));
        assert_eq!(gripping.toward(true), None);
        assert_eq!(gripping.toward(false), Some(Grip::LetGo));
        assert_eq!(gripping.toward(false), None);
    }
}
