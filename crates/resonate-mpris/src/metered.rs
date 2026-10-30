use std::{collections::HashMap, mem::ManuallyDrop, thread, time::Duration};

use resonate_core::{Meter, Metering};
use zbus::{
    OwnedVariant,
    names::{BusName, OwnedWellKnownName, WellKnownName},
    proxy::SignalStream,
};

use crate::{
    BusOp, Error, Result,
    blocking::{Arrivals, Connection, Proxy, connection, fdo::DBusProxy},
};

const NETWORK_MANAGER: &str = "org.freedesktop.NetworkManager";
const MANAGER_PATH: &str = "/org/freedesktop/NetworkManager";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const PROPERTIES_CHANGED: &str = "PropertiesChanged";
const GET: &str = "Get";
const METERED: &str = "Metered";
const ANSWERS_WITHIN: Duration = Duration::from_secs(2);
const ASKED_AGAIN_AFTER: Duration = Duration::from_secs(60);
const YES: u32 = 1;
const NO: u32 = 2;
const GUESS_YES: u32 = 3;
const GUESS_NO: u32 = 4;

pub fn meter_of_network_manager(value: u32) -> Meter {
    match value {
        YES | GUESS_YES => Meter::Metered,
        NO | GUESS_NO => Meter::Unmetered,
        _ => Meter::Unknown,
    }
}

pub fn watch_the_meter(metering: Metering) -> Result<()> {
    let connection = connected()?;
    let first = Following::begin(&connection, &metering);

    thread::Builder::new()
        .name("resonate-metered".to_owned())
        .spawn(move || keep_watching(&connection, &metering, first))
        .map(drop)
        .map_err(|_| Error::ThreadStopped)
}

fn connected() -> Result<Connection> {
    connection::Builder::system()
        .map_err(|source| Error::bus(BusOp::Connect, source))?
        .method_timeout(ANSWERS_WITHIN)
        .build()
        .map_err(|source| Error::bus(BusOp::Connect, source))
}

fn keep_watching(connection: &Connection, metering: &Metering, mut following: Result<Following>) {
    loop {
        match following {
            Ok(following) => following.follow(),
            Err(error) => {
                tracing::debug!(%error, "NetworkManager did not say whether the connection is metered, so it is taken as not");
            }
        }
        metering.report(Meter::Unknown);
        thread::sleep(ASKED_AGAIN_AFTER);
        following = Following::begin(connection, metering);
    }
}

struct Following {
    properties: Proxy<'static>,
    changes: ManuallyDrop<Arrivals<SignalStream<'static>>>,
    metering: Metering,
}

impl Following {
    fn begin(connection: &Connection, metering: &Metering) -> Result<Self> {
        if !network_manager_is_running(connection) {
            return Err(Error::NobodyOwns {
                name: OwnedWellKnownName::from(WellKnownName::from_static_str_unchecked(
                    NETWORK_MANAGER,
                )),
            });
        }
        let properties = Proxy::new(connection, NETWORK_MANAGER, MANAGER_PATH, PROPERTIES)
            .map_err(|source| Error::bus(BusOp::Connect, source))?;
        let changes = properties
            .receive_signal(PROPERTIES_CHANGED)
            .map_err(|source| Error::bus(BusOp::Listen, source))?;
        metering.report(read(&properties)?);

        Ok(Self {
            properties,
            changes: ManuallyDrop::new(changes),
            metering: metering.clone(),
        })
    }

    fn follow(mut self) {
        for change in self.changes.by_ref() {
            let body = change.body();
            let Ok((interface, changed, invalidated)) =
                body.deserialize::<(String, HashMap<String, OwnedVariant>, Vec<String>)>()
            else {
                continue;
            };
            if interface != NETWORK_MANAGER {
                continue;
            }
            if let Some(value) = changed.get(METERED).and_then(whole) {
                self.metering.report(meter_of_network_manager(value));
            } else if invalidated.iter().any(|name| name == METERED) {
                match read(&self.properties) {
                    Ok(meter) => self.metering.report(meter),
                    Err(error) => {
                        tracing::debug!(%error, "NetworkManager was not asked again whether the connection is metered");
                        return;
                    }
                }
            }
        }
    }
}

fn network_manager_is_running(connection: &Connection) -> bool {
    let Ok(name) = BusName::try_from(NETWORK_MANAGER) else {
        return false;
    };
    DBusProxy::new(connection)
        .ok()
        .and_then(|bus| bus.name_has_owner(name).ok())
        .unwrap_or(false)
}

fn read(properties: &Proxy<'_>) -> Result<Meter> {
    let value = properties
        .call::<_, _, OwnedVariant>(GET, &(NETWORK_MANAGER, METERED))
        .map_err(|source| Error::bus(BusOp::Read, source))?;
    Ok(whole(&value).map_or(Meter::Unknown, meter_of_network_manager))
}

fn whole(value: &OwnedVariant) -> Option<u32> {
    u32::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_manager_says_yes_and_guess_yes_for_a_metered_connection() {
        assert_eq!(meter_of_network_manager(1), Meter::Metered);
        assert_eq!(meter_of_network_manager(3), Meter::Metered);
    }

    #[test]
    fn network_manager_says_no_and_guess_no_for_an_unmetered_one() {
        assert_eq!(meter_of_network_manager(2), Meter::Unmetered);
        assert_eq!(meter_of_network_manager(4), Meter::Unmetered);
    }

    #[test]
    fn what_network_manager_does_not_know_or_a_number_it_never_sends_is_unknown() {
        assert_eq!(meter_of_network_manager(0), Meter::Unknown);
        assert_eq!(meter_of_network_manager(5), Meter::Unknown);
        assert_eq!(meter_of_network_manager(u32::MAX), Meter::Unknown);
    }
}
