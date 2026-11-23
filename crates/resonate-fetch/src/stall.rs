use std::time::Duration;

use ureq::{
    Error, Timeout,
    unversioned::transport::{Buffers, ConnectionDetails, Connector, NextTimeout, Transport},
};

#[derive(Debug)]
pub struct BrokenOffAfter(pub Duration);

impl Connector<Box<dyn Transport>> for BrokenOffAfter {
    type Out = Stalling;

    fn connect(
        &self,
        _details: &ConnectionDetails,
        chained: Option<Box<dyn Transport>>,
    ) -> Result<Option<Self::Out>, Error> {
        Ok(chained.map(|inner| Stalling {
            inner,
            broken_off_after: self.0,
        }))
    }
}

#[derive(Debug)]
pub struct Stalling {
    inner: Box<dyn Transport>,
    broken_off_after: Duration,
}

fn at_most(timeout: NextTimeout, longest: Duration) -> NextTimeout {
    if timeout.reason == Timeout::RecvResponse || *timeout.after <= longest {
        return timeout;
    }
    NextTimeout {
        after: longest.into(),
        reason: Timeout::RecvBody,
        ..timeout
    }
}

impl Transport for Stalling {
    fn buffers(&mut self) -> &mut dyn Buffers {
        self.inner.buffers()
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), Error> {
        self.inner.transmit_output(amount, timeout)
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, Error> {
        self.inner
            .await_input(at_most(timeout, self.broken_off_after))
    }

    fn is_open(&mut self) -> bool {
        self.inner.is_open()
    }

    fn is_tls(&self) -> bool {
        self.inner.is_tls()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::{Ipv4Addr, TcpListener},
        thread,
        time::Instant,
    };

    use ureq::{Agent, unversioned::transport::time};

    use super::*;
    use crate::{Trusted, configured, downloading_agent};

    fn waiting_for(after: Duration, reason: Timeout) -> NextTimeout {
        NextTimeout {
            after: after.into(),
            reason,
            ..NextTimeout::default()
        }
    }

    fn slow_to_begin_then_quiet(
        before_the_head: Duration,
        quiet_for: Duration,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("binds");
        let url = format!("http://{}/song", listener.local_addr().expect("an address"));

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("a connection");
            let mut head = [0; 1024];

            let _ = stream.read(&mut head);

            thread::sleep(before_the_head);

            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc");

            thread::sleep(quiet_for);
        });
        (url, server)
    }

    fn agent_waiting(response: Duration, body: Duration) -> Agent {
        downloading_agent(configured(response, Trusted::BuiltInRoots).build(), body)
    }

    #[test]
    fn a_response_phase_limit_longer_than_the_stall_is_kept_and_the_body_phase_is_capped() {
        let response = waiting_for(Duration::from_secs(60), Timeout::RecvResponse);
        let body = waiting_for(Duration::from_secs(60), Timeout::RecvBody);

        let kept = at_most(response, Duration::from_secs(30));
        let capped = at_most(body, Duration::from_secs(30));

        assert_eq!(*kept.after, Duration::from_secs(60));
        assert_eq!(kept.reason, Timeout::RecvResponse);
        assert_eq!(*capped.after, Duration::from_secs(30));
        assert_eq!(capped.reason, Timeout::RecvBody);
    }

    #[test]
    fn a_first_response_between_the_stall_and_the_response_limit_opens() {
        let (url, server) =
            slow_to_begin_then_quiet(Duration::from_millis(900), Duration::from_millis(100));
        let agent = agent_waiting(Duration::from_secs(5), Duration::from_millis(300));

        let response = agent.get(&url).call().expect("the answer is waited for");

        assert_eq!(response.status(), 200);

        server.join().expect("the server ends");
    }

    #[test]
    fn a_response_never_coming_ends_at_the_response_limit() {
        let (url, server) = slow_to_begin_then_quiet(Duration::from_millis(1_500), Duration::ZERO);
        let agent = agent_waiting(Duration::from_millis(600), Duration::from_millis(200));
        let started = Instant::now();

        let error = agent.get(&url).call().expect_err("no answer comes");

        assert!(
            matches!(error, Error::Timeout(Timeout::RecvResponse)),
            "{error}"
        );
        assert!(started.elapsed() >= Duration::from_millis(500));

        server.join().expect("the server ends");
    }

    #[test]
    fn a_body_going_quiet_is_broken_off_at_the_stall_though_the_response_limit_is_longer() {
        let (url, server) = slow_to_begin_then_quiet(Duration::ZERO, Duration::from_millis(1_500));
        let agent = agent_waiting(Duration::from_secs(10), Duration::from_millis(400));
        let started = Instant::now();

        let mut response = agent.get(&url).call().expect("the head arrives");

        let error = response
            .body_mut()
            .read_to_vec()
            .expect_err("the body goes quiet");

        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(error.to_string().contains("timeout"), "{error}");

        server.join().expect("the server ends");
    }

    #[test]
    fn a_read_with_no_deadline_of_its_own_is_broken_off_after_the_stall() {
        let unbounded = NextTimeout {
            after: time::Duration::NotHappening,
            reason: Timeout::RecvBody,
            ..NextTimeout::default()
        };
        let sooner = NextTimeout {
            after: Duration::from_secs(1).into(),
            reason: Timeout::Global,
            ..NextTimeout::default()
        };

        let capped = at_most(unbounded, Duration::from_secs(30));
        let kept = at_most(sooner, Duration::from_secs(30));

        assert_eq!(*capped.after, Duration::from_secs(30));
        assert_eq!(capped.reason, Timeout::RecvBody);
        assert_eq!(*kept.after, Duration::from_secs(1));
        assert_eq!(kept.reason, Timeout::Global);
    }
}
