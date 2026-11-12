use resonate_engine::{SinkInfo, desktop_backend};

use crate::{Result, sinks::SINK_TIMEOUT};

pub(crate) const APP_NAME: &str = "Resonate";

pub(crate) fn sinks() -> Result<Vec<SinkInfo>> {
    let backend = desktop_backend(APP_NAME)?;
    let found = backend.surveyor().enumerate_sinks(SINK_TIMEOUT);
    backend.shutdown()?;
    Ok(found?)
}
