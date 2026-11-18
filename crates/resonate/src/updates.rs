#[cfg(feature = "ui")]
use std::sync::Arc;
use std::time::SystemTime;

#[cfg(feature = "ui")]
use resonate_core::Switch;
use resonate_online::KeptRelease;

use crate::{
    Result,
    config::{self, Config},
    online::{self, Asking},
    words,
};

const THIS_BUILD: &str = env!("CARGO_PKG_VERSION");

fn kept() -> Result<KeptRelease> {
    Ok(KeptRelease::in_folder(&config::data_dir()?))
}

pub fn run(config: &Config) -> Result<()> {
    let latest = online::latest_release(config, Asking::ForTheListener)?;
    if let Ok(kept) = kept() {
        kept.heard_now(latest.clone(), SystemTime::now());
    }

    said!("{}", words::updates::this_build(THIS_BUILD));
    match latest {
        None => said!("{}", words::updates::none_published()),
        Some(latest) if latest.version.outdates_this_build() => {
            said!("{}", words::updates::newer(latest.version.to_string()));
            said!("{}", latest.page);
        }
        Some(latest) => said!("{}", words::updates::not_behind(latest.version.to_string())),
    }
    Ok(())
}

#[cfg(feature = "ui")]
struct Checking {
    config: Config,
}

#[cfg(feature = "ui")]
impl resonate_ui::Updates for Checking {
    fn newer(&self, reaching: bool) -> Option<resonate_ui::Newer> {
        let kept = kept()
            .map_err(
                |error| tracing::debug!(%error, "there is no folder to keep the latest release in"),
            )
            .ok()?;
        kept.newer_lately(SystemTime::now(), reaching, || {
            online::latest_release(&self.config, Asking::InTheBackground)
        })
        .map(|released| resonate_ui::Newer {
            version: released.version.to_string(),
            page: released.page,
        })
    }
}

#[cfg(feature = "ui")]
pub fn checking(config: &Config) -> Option<Arc<dyn resonate_ui::Updates>> {
    online::names_where_it_was_released().then(|| {
        Arc::new(Checking {
            config: Config {
                switches: config.switches.with(Switch::Online, true),
                ..config.clone()
            },
        }) as Arc<dyn resonate_ui::Updates>
    })
}
