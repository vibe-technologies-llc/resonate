use std::time::Duration;

use ureq::{
    Agent,
    config::{Config, ConfigBuilder},
    typestate::AgentScope,
    unversioned::{
        resolver::DefaultResolver,
        transport::{Connector as _, DefaultConnector},
    },
};

use crate::{stall::BrokenOffAfter, trust};

pub const USER_AGENT: &str = concat!("resonate/", env!("CARGO_PKG_VERSION"));
pub const CONNECTED_WITHIN: Duration = Duration::from_secs(10);
const ANSWERED_WITHIN: Duration = Duration::from_secs(20);
const BROKEN_OFF_AFTER: Duration = Duration::from_secs(30);
const RESUMED_AFTER: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Patience {
    pub answered_within: Duration,
    pub broken_off_after: Duration,
    pub resumed_after: Duration,
}

impl Default for Patience {
    fn default() -> Self {
        Self {
            answered_within: ANSWERED_WITHIN,
            broken_off_after: BROKEN_OFF_AFTER,
            resumed_after: RESUMED_AFTER,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trusted {
    BuiltInRoots,
    SystemStoreToo,
}

pub fn configured(answered_within: Duration, trusted: Trusted) -> ConfigBuilder<AgentScope> {
    let builder = Agent::config_builder()
        .user_agent(USER_AGENT)
        .timeout_connect(Some(CONNECTED_WITHIN))
        .timeout_recv_response(Some(answered_within))
        .http_status_as_error(false);
    match trusted {
        Trusted::BuiltInRoots => builder,
        Trusted::SystemStoreToo => builder.tls_config(trust::system_and_built_in()),
    }
}

pub fn asking_agent(patience: Patience, trusted: Trusted) -> Agent {
    configured(patience.answered_within, trusted)
        .timeout_global(Some(patience.answered_within))
        .build()
        .new_agent()
}

pub fn downloading_agent(config: Config, broken_off_after: Duration) -> Agent {
    Agent::with_parts(
        config,
        DefaultConnector::new().chain(BrokenOffAfter(broken_off_after)),
        DefaultResolver::default(),
    )
}
