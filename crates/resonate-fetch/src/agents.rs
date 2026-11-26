use std::{
    sync::{
        Arc,
        mpsc::{self, RecvTimeoutError},
    },
    thread,
    time::Duration,
};

use ureq::{
    Agent, Error,
    config::{Config, ConfigBuilder},
    http::Uri,
    typestate::AgentScope,
    unversioned::{
        resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver},
        transport::{Connector as _, DefaultConnector, NextTimeout, time},
    },
};

use crate::{ranged::UNCODED, stall::BrokenOffAfter, trust};

pub const USER_AGENT: &str = "resonate";
pub const CONNECTED_WITHIN: Duration = Duration::from_secs(10);
const RESOLVED_WITHIN: Duration = Duration::from_secs(10);
const ANSWERED_WITHIN: Duration = Duration::from_secs(20);
const BROKEN_OFF_AFTER: Duration = Duration::from_secs(30);
const RESUMED_AFTER: Duration = Duration::from_secs(1);
const LONGEST_RETRY_AFTER: Duration = Duration::from_secs(8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Patience {
    pub answered_within: Duration,
    pub broken_off_after: Duration,
    pub resumed_after: Duration,
    pub longest_retry_after: Duration,
}

impl Default for Patience {
    fn default() -> Self {
        Self {
            answered_within: ANSWERED_WITHIN,
            broken_off_after: BROKEN_OFF_AFTER,
            resumed_after: RESUMED_AFTER,
            longest_retry_after: LONGEST_RETRY_AFTER,
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
        .timeout_resolve(Some(RESOLVED_WITHIN))
        .timeout_connect(Some(CONNECTED_WITHIN))
        .timeout_recv_response(Some(answered_within))
        .max_redirects(0)
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

#[derive(Debug)]
struct Bounded<R>(Arc<R>);

impl<R: Resolver> Resolver for Bounded<R> {
    fn resolve(
        &self,
        uri: &Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, Error> {
        if timeout.after.is_not_happening() {
            return self.0.resolve(uri, config, timeout);
        }

        let (sender, receiver) = mpsc::sync_channel(1);
        let resolver = Arc::clone(&self.0);
        let uri = uri.clone();
        let config = config.clone();
        let unbounded = NextTimeout {
            after: time::Duration::NotHappening,
            ..timeout
        };

        thread::spawn(move || sender.send(resolver.resolve(&uri, &config, unbounded)).ok());

        match receiver.recv_timeout(*timeout.after) {
            Ok(resolved) => resolved,
            Err(RecvTimeoutError::Timeout) => Err(Error::Timeout(timeout.reason)),
            Err(RecvTimeoutError::Disconnected) => Err(Error::HostNotFound),
        }
    }
}

fn downloading_through(
    config: ConfigBuilder<AgentScope>,
    broken_off_after: Duration,
    resolver: impl Resolver,
) -> Agent {
    Agent::with_parts(
        config.accept_encoding(UNCODED).build(),
        DefaultConnector::new().chain(BrokenOffAfter(broken_off_after)),
        Bounded(Arc::new(resolver)),
    )
}

pub fn downloading_agent(config: ConfigBuilder<AgentScope>, broken_off_after: Duration) -> Agent {
    downloading_through(config, broken_off_after, DefaultResolver::default())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{ErrorKind, Read},
        sync::atomic::{AtomicUsize, Ordering},
        time::Instant,
    };

    use ureq::Timeout;

    use super::*;
    use crate::{
        Ranged,
        testing::{WHOLE_BODY, cut_first_answer, serving},
    };

    const RESOLUTION_BUDGET: Duration = Duration::from_millis(300);
    const STUCK_FOR: Duration = Duration::from_secs(20);
    const NO_LONGER_THAN: Duration = Duration::from_secs(5);

    #[derive(Debug)]
    struct Slow {
        asked: AtomicUsize,
        slow_from: usize,
        delay: Duration,
    }

    impl Slow {
        fn new(slow_from: usize, delay: Duration) -> Self {
            Self {
                asked: AtomicUsize::new(0),
                slow_from,
                delay,
            }
        }
    }

    impl Resolver for Slow {
        fn resolve(
            &self,
            uri: &Uri,
            config: &Config,
            timeout: NextTimeout,
        ) -> Result<ResolvedSocketAddrs, Error> {
            if self.asked.fetch_add(1, Ordering::SeqCst) >= self.slow_from {
                thread::sleep(self.delay);
            }
            DefaultResolver::default().resolve(uri, config, timeout)
        }
    }

    fn budgeted() -> ConfigBuilder<AgentScope> {
        configured(Duration::from_secs(5), Trusted::BuiltInRoots)
            .timeout_resolve(Some(RESOLUTION_BUDGET))
    }

    fn is_resolution_timeout(error: &Error) -> bool {
        matches!(error, Error::Timeout(Timeout::Resolve))
    }

    #[test]
    fn a_new_request_stuck_resolving_the_host_is_given_up_at_the_resolution_budget() {
        let (url, _) = serving(Vec::new());
        let agent = downloading_through(budgeted(), BROKEN_OFF_AFTER, Slow::new(0, STUCK_FOR));
        let started = Instant::now();

        let error = agent.get(&url).call().expect_err("the lookup never ends");

        assert!(is_resolution_timeout(&error), "{error}");
        assert!(started.elapsed() < NO_LONGER_THAN);
    }

    #[test]
    fn a_reopened_download_stuck_resolving_the_host_is_given_up_at_the_resolution_budget() {
        let (url, heads) = serving(vec![Box::new(|_| cut_first_answer(&[]))]);
        let agent = downloading_through(budgeted(), BROKEN_OFF_AFTER, Slow::new(1, STUCK_FOR));

        let response = agent.get(&url).call().expect("the first answer");
        let mut ranged = Ranged::continuing(agent, url, response, Duration::ZERO);
        let mut read = Vec::new();
        let started = Instant::now();

        let error = ranged
            .read_to_end(&mut read)
            .expect_err("the reopening never resolves");

        assert_eq!(read, &WHOLE_BODY[..20]);
        assert_eq!(error.kind(), ErrorKind::TimedOut);
        assert!(started.elapsed() < NO_LONGER_THAN);
        assert_eq!(heads.lock().len(), 1);
    }

    #[test]
    fn a_download_making_progress_runs_past_the_resolution_budget() {
        let (url, _) = serving(vec![Box::new(|_| {
            thread::sleep(RESOLUTION_BUDGET * 2);
            crate::testing::answering("200 OK", &[], WHOLE_BODY, WHOLE_BODY.len())
        })]);
        let agent = downloading_through(
            budgeted(),
            BROKEN_OFF_AFTER,
            Slow::new(usize::MAX, STUCK_FOR),
        );
        let started = Instant::now();

        let mut response = agent.get(&url).call().expect("the answer arrives");

        let body = response.body_mut().read_to_vec().expect("the body is read");

        assert_eq!(body, WHOLE_BODY);
        assert!(started.elapsed() > RESOLUTION_BUDGET);
    }
}
