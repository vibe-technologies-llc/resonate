use std::{thread, time::Duration};

use crossbeam_channel::bounded;
use resonate_apple::{PathMonitor, Route};
use resonate_core::{Meter, Metering};

use crate::{
    Error, Result,
    route::{Path, meter_of_the_path},
};

const SETTLES_WITHIN: Duration = Duration::from_secs(5);

pub fn watch_the_meter(metering: Metering) -> Result<()> {
    let (settled, hears) = bounded(1);

    thread::Builder::new()
        .name("resonate-metered".to_owned())
        .spawn(move || match following(metering) {
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

fn following(metering: Metering) -> Result<PathMonitor> {
    metering.report(Meter::Unknown);
    PathMonitor::following(move |route| metering.report(meter_of(route)))
        .map_err(|source| Error::Path { source })
}

fn meter_of(route: Route) -> Meter {
    meter_of_the_path(Path {
        reachable: route.reachable,
        expensive: route.expensive,
        constrained: route.constrained,
    })
}
