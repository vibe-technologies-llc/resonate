use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::Mutex;

const ATTEMPT_WINDOW: Duration = Duration::from_secs(60);
const SPACE_WINDOW: Duration = Duration::from_secs(24 * 60 * 60);
const FORGET_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
const MOST_ADDRESSES_KEPT: usize = 100_000;
const IPV6_PREFIX_BYTES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allowance {
    pub connections: usize,
    pub connections_per_address: usize,
    pub attempts_per_minute: u32,
    pub spaces_per_address_per_day: u32,
}

impl Default for Allowance {
    fn default() -> Self {
        Self {
            connections: 2048,
            connections_per_address: 64,
            attempts_per_minute: 30,
            spaces_per_address_per_day: 5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Source(IpAddr);

struct Account {
    open: usize,
    attempts: u32,
    attempts_since: Instant,
    spaces: u32,
    spaces_since: Instant,
    seen: Instant,
}

#[derive(Default)]
struct Ledger {
    accounts: HashMap<Source, Account>,
    open: usize,
}

pub struct Guard {
    allowance: Allowance,
    ledger: Mutex<Ledger>,
}

pub struct Admitted {
    guard: Arc<Guard>,
    source: Source,
}

impl Source {
    fn of(address: IpAddr) -> Self {
        match address.to_canonical() {
            IpAddr::V6(v6) => {
                let mut octets = v6.octets();
                octets[IPV6_PREFIX_BYTES..].fill(0);
                Self(IpAddr::V6(octets.into()))
            }
            canonical => Self(canonical),
        }
    }
}

impl Account {
    fn new(now: Instant) -> Self {
        Self {
            open: 0,
            attempts: 0,
            attempts_since: now,
            spaces: 0,
            spaces_since: now,
            seen: now,
        }
    }
}

impl Guard {
    pub fn new(allowance: Allowance) -> Arc<Self> {
        Arc::new(Self {
            allowance,
            ledger: Mutex::new(Ledger::default()),
        })
    }

    pub fn admit(self: &Arc<Self>, address: IpAddr) -> Option<Arc<Admitted>> {
        let source = Source::of(address);
        let now = Instant::now();
        let mut ledger = self.ledger.lock();
        if ledger.open >= self.allowance.connections {
            return None;
        }
        if ledger.accounts.len() >= MOST_ADDRESSES_KEPT && !ledger.accounts.contains_key(&source) {
            ledger.forget_quiet(now);
            if ledger.accounts.len() >= MOST_ADDRESSES_KEPT {
                return None;
            }
        }
        let account = ledger
            .accounts
            .entry(source)
            .or_insert_with(|| Account::new(now));
        if now.duration_since(account.attempts_since) >= ATTEMPT_WINDOW {
            account.attempts_since = now;
            account.attempts = 0;
        }
        account.seen = now;
        if account.attempts >= self.allowance.attempts_per_minute
            || account.open >= self.allowance.connections_per_address
        {
            return None;
        }
        account.attempts += 1;
        account.open += 1;
        ledger.open += 1;
        Some(Arc::new(Admitted {
            guard: Arc::clone(self),
            source,
        }))
    }

    fn release(&self, source: Source) {
        let mut ledger = self.ledger.lock();
        ledger.open = ledger.open.saturating_sub(1);
        if let Some(account) = ledger.accounts.get_mut(&source) {
            account.open = account.open.saturating_sub(1);
        }
    }

    pub fn forget_quiet_addresses(&self) {
        self.ledger.lock().forget_quiet(Instant::now());
    }
}

impl Ledger {
    fn forget_quiet(&mut self, now: Instant) {
        self.accounts.retain(|_, account| {
            account.open > 0 || now.duration_since(account.seen) < FORGET_AFTER
        });
    }
}

impl Admitted {
    pub fn may_make_a_space(&self) -> bool {
        let now = Instant::now();
        let mut ledger = self.guard.ledger.lock();
        let Some(account) = ledger.accounts.get_mut(&self.source) else {
            return false;
        };
        if now.duration_since(account.spaces_since) >= SPACE_WINDOW {
            account.spaces_since = now;
            account.spaces = 0;
        }
        if account.spaces >= self.guard.allowance.spaces_per_address_per_day {
            return false;
        }
        account.spaces += 1;
        true
    }
}

impl Drop for Admitted {
    fn drop(&mut self) {
        self.guard.release(self.source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(last: u8) -> IpAddr {
        IpAddr::from([203, 0, 113, last])
    }

    fn tight() -> Allowance {
        Allowance {
            connections: 4,
            connections_per_address: 2,
            attempts_per_minute: 3,
            spaces_per_address_per_day: 1,
        }
    }

    #[test]
    fn one_address_holds_only_so_many_connections_and_gets_them_back_on_closing() {
        let guard = Guard::new(tight());

        let first = guard.admit(address(1));
        let second = guard.admit(address(1));

        assert!(first.is_some() && second.is_some());
        assert!(guard.admit(address(1)).is_none());
        assert!(guard.admit(address(2)).is_some());

        drop(first);

        assert!(guard.admit(address(1)).is_some());
    }

    #[test]
    fn an_address_is_held_to_so_many_attempts_a_minute_however_briefly_each_lasts() {
        let guard = Guard::new(tight());

        for _ in 0..3 {
            assert!(guard.admit(address(1)).is_some());
        }

        assert!(guard.admit(address(1)).is_none());
        assert!(guard.admit(address(2)).is_some());
    }

    #[test]
    fn every_address_together_holds_only_so_many_connections() {
        let guard = Guard::new(tight());

        let held = (1..=4)
            .map(|last| guard.admit(address(last)))
            .collect::<Vec<_>>();

        assert!(held.iter().all(Option::is_some));
        assert!(guard.admit(address(5)).is_none());
    }

    #[test]
    fn an_address_makes_only_so_many_spaces_a_day() {
        let guard = Guard::new(tight());
        let admitted = guard.admit(address(1)).expect("admitted");

        assert!(admitted.may_make_a_space());
        assert!(!admitted.may_make_a_space());

        let other = guard.admit(address(2)).expect("admitted");

        assert!(other.may_make_a_space());
    }

    #[test]
    fn a_whole_sixty_four_bit_prefix_is_one_address() {
        let guard = Guard::new(tight());
        let within = |last: u16| IpAddr::from([0x2001, 0xdb8, 0, 1, 0, 0, 0, last]);

        let held = [guard.admit(within(1)), guard.admit(within(2))];

        assert!(held.iter().all(Option::is_some));
        assert!(guard.admit(within(3)).is_none());
    }

    #[test]
    fn an_address_mapped_into_six_is_the_same_as_the_four_it_wraps() {
        let guard = Guard::new(tight());
        let mapped = IpAddr::from([0, 0, 0, 0, 0, 0xffff, 0xcb00, 0x7101]);

        let held = [guard.admit(address(1)), guard.admit(mapped)];

        assert!(held.iter().all(Option::is_some));
        assert!(guard.admit(address(1)).is_none());
    }
}
