use std::sync::{LazyLock, Once};

use resonate_core::{Gate, Metering, Switch};

use crate::config::Config;

static METERING: LazyLock<Metering> = LazyLock::new(Metering::new);
static WATCHING: Once = Once::new();
#[cfg(feature = "online")]
static LOOKUPS: LazyLock<Gate> = LazyLock::new(|| Gate::new(METERING.clone(), true));
static DOWNLOADS: LazyLock<Gate> = LazyLock::new(|| Gate::new(METERING.clone(), false));

fn watch_the_connection() {
    WATCHING.call_once(|| {
        if let Err(error) = resonate_mpris::watch_the_meter(METERING.clone()) {
            tracing::debug!(%error, "whether the connection is metered is not watched, so it is taken as not");
        }
    });
}

fn shut_while_metered(gate: &Gate, shut: bool) {
    gate.shut_while_metered(shut);
    if shut {
        watch_the_connection();
    }
}

#[cfg(feature = "online")]
pub fn for_lookups(config: &Config) -> Gate {
    shut_while_metered(&LOOKUPS, !config.is_on(Switch::EnrichOnMetered));
    LOOKUPS.clone()
}

#[cfg(all(feature = "ui", feature = "online"))]
pub fn allow_lookups(allowed: bool) {
    shut_while_metered(&LOOKUPS, !allowed);
}

#[cfg(feature = "ui")]
pub fn stall_downloads(stalls: bool) {
    shut_while_metered(&DOWNLOADS, stalls);
}

pub fn for_downloads(config: &Config) -> Gate {
    shut_while_metered(&DOWNLOADS, config.is_on(Switch::StallDownloadsOnMetered));
    DOWNLOADS.clone()
}

#[cfg(test)]
mod tests {
    use resonate_core::{Meter, Switch, Switches};

    use super::*;

    #[test]
    fn a_build_with_no_settings_stops_lookups_on_a_metered_connection_and_holds_no_download() {
        let config = Config::default();
        let metering = Metering::new();
        let lookups = Gate::new(metering.clone(), !config.is_on(Switch::EnrichOnMetered));
        let downloads = Gate::new(
            metering.clone(),
            config.is_on(Switch::StallDownloadsOnMetered),
        );

        metering.report(Meter::Metered);

        assert!(lookups.is_shut());
        assert!(!downloads.is_shut());
    }

    #[test]
    fn the_settings_open_and_shut_the_gates_both_ways() {
        let config = Config {
            switches: Switches::UNSAID
                .with(Switch::EnrichOnMetered, true)
                .with(Switch::StallDownloadsOnMetered, true),
            ..Config::default()
        };
        let metering = Metering::new();
        let lookups = Gate::new(metering.clone(), !config.is_on(Switch::EnrichOnMetered));
        let downloads = Gate::new(
            metering.clone(),
            config.is_on(Switch::StallDownloadsOnMetered),
        );

        metering.report(Meter::Metered);

        assert!(!lookups.is_shut());
        assert!(downloads.is_shut());

        metering.report(Meter::Unmetered);

        assert!(!downloads.is_shut());
    }
}
