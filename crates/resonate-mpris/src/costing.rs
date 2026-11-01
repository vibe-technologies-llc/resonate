use std::{thread, time::Duration};

use crossbeam_channel::bounded;
use resonate_core::{Meter, Metering};
use windows::Networking::Connectivity::{
    ConnectionProfile, NetworkInformation, NetworkStatusChangedEventHandler,
};

use crate::{Error, Result, WindowsOp, apartment::Apartment, cost::meter_of_network_cost};

const SETTLES_WITHIN: Duration = Duration::from_secs(5);

pub fn watch_the_meter(metering: Metering) -> Result<()> {
    let (settled, hears) = bounded(1);

    thread::Builder::new()
        .name("resonate-metered".to_owned())
        .spawn(move || match following(&metering) {
            Ok(_held) => {
                let _ = settled.send(Ok(()));
                loop {
                    thread::park();
                }
            }
            Err(error) => {
                let _ = settled.send(Err(error));
            }
        })
        .map_err(|_| Error::ThreadStopped)?;

    hears
        .recv_timeout(SETTLES_WITHIN)
        .map_err(|_| Error::ThreadStopped)?
}

fn following(metering: &Metering) -> Result<(Apartment, i64)> {
    let apartment = Apartment::enter()?;
    let reporting = metering.clone();
    let handler = NetworkStatusChangedEventHandler::new(move |_| {
        report(&reporting);
        Ok(())
    });
    let registration = NetworkInformation::NetworkStatusChanged(&handler)
        .map_err(|source| Error::windows(WindowsOp::Watch, source))?;
    report(metering);

    Ok((apartment, registration))
}

fn report(metering: &Metering) {
    match read() {
        Ok(meter) => metering.report(meter),
        Err(error) => {
            tracing::debug!(%error, "Windows did not say whether the connection is metered, so it is taken as not");
            metering.report(Meter::Unknown);
        }
    }
}

fn read() -> Result<Meter> {
    let Some(profile) = profile()? else {
        return Ok(Meter::Unknown);
    };
    let cost = profile
        .GetConnectionCost()
        .map_err(|source| Error::windows(WindowsOp::Cost, source))?;
    let kind = cost
        .NetworkCostType()
        .map_err(|source| Error::windows(WindowsOp::Cost, source))?;

    Ok(meter_of_network_cost(kind.0))
}

fn profile() -> Result<Option<ConnectionProfile>> {
    match NetworkInformation::GetInternetConnectionProfile() {
        Ok(profile) => Ok(Some(profile)),
        Err(source) if source.code().is_ok() => Ok(None),
        Err(source) => Err(Error::windows(WindowsOp::Profile, source)),
    }
}
