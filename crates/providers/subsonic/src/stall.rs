use std::time::Duration;

use ureq::{
    Error, Timeout,
    unversioned::transport::{Buffers, ConnectionDetails, Connector, NextTimeout, Transport},
};

#[derive(Debug)]
pub(crate) struct BrokenOffAfter(pub(crate) Duration);

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
pub(crate) struct Stalling {
    inner: Box<dyn Transport>,
    broken_off_after: Duration,
}

fn at_most(timeout: NextTimeout, longest: Duration) -> NextTimeout {
    if *timeout.after <= longest {
        return timeout;
    }
    NextTimeout {
        after: longest.into(),
        reason: Timeout::RecvBody,
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
    use ureq::unversioned::transport::time;

    use super::*;

    #[test]
    fn a_read_with_no_deadline_of_its_own_is_broken_off_after_the_stall() {
        let unbounded = NextTimeout {
            after: time::Duration::NotHappening,
            reason: Timeout::RecvBody,
        };
        let sooner = NextTimeout {
            after: Duration::from_secs(1).into(),
            reason: Timeout::Global,
        };

        let capped = at_most(unbounded, Duration::from_secs(30));
        let kept = at_most(sooner, Duration::from_secs(30));

        assert_eq!(*capped.after, Duration::from_secs(30));
        assert_eq!(capped.reason, Timeout::RecvBody);
        assert_eq!(*kept.after, Duration::from_secs(1));
        assert_eq!(kept.reason, Timeout::Global);
    }
}
