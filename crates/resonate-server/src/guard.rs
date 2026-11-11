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
const ADDRESSES_EVICTED_AT_ONCE_DIVISOR: usize = 100;
const IPV6_PREFIX_BYTES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Allowance {
    pub connections: usize,
    pub connections_per_address: usize,
    pub attempts_per_minute: u32,
    pub spaces_per_address_per_day: u32,
    pub shared_spaces_per_address_per_day: u32,
    pub rooms_per_address_per_day: u32,
}

impl Default for Allowance {
    fn default() -> Self {
        Self {
            connections: 2048,
            connections_per_address: 64,
            attempts_per_minute: 30,
            spaces_per_address_per_day: 5,
            shared_spaces_per_address_per_day: 50,
            rooms_per_address_per_day: 50,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Source(IpAddr);

#[derive(Clone, Copy)]
struct Allowed {
    spent: u32,
    since: Instant,
}

impl Allowed {
    const fn new(now: Instant) -> Self {
        Self {
            spent: 0,
            since: now,
        }
    }

    fn spend(&mut self, most: u32, now: Instant) -> bool {
        if now.duration_since(self.since) >= SPACE_WINDOW {
            self.since = now;
            self.spent = 0;
        }
        if self.spent >= most {
            return false;
        }
        self.spent += 1;
        true
    }
}

struct Account {
    open: usize,
    attempts: u32,
    attempts_since: Instant,
    spaces: Allowed,
    shared_spaces: Allowed,
    rooms: Allowed,
    seen: Instant,
}

#[derive(Default)]
struct Ledger {
    accounts: HashMap<Source, Account>,
    open: usize,
}

pub struct Guard {
    allowance: Allowance,
    most_addresses: usize,
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
            spaces: Allowed::new(now),
            shared_spaces: Allowed::new(now),
            rooms: Allowed::new(now),
            seen: now,
        }
    }
}

impl Guard {
    pub fn new(allowance: Allowance) -> Arc<Self> {
        Self::keeping(allowance, MOST_ADDRESSES_KEPT)
    }

    fn keeping(allowance: Allowance, most_addresses: usize) -> Arc<Self> {
        Arc::new(Self {
            allowance,
            most_addresses,
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
        if ledger.accounts.len() >= self.most_addresses && !ledger.accounts.contains_key(&source) {
            ledger.forget_quiet(now);
            ledger.evict_the_least_recently_seen(self.most_addresses);
            if ledger.accounts.len() >= self.most_addresses {
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
    fn evict_the_least_recently_seen(&mut self, most: usize) {
        if self.accounts.len() < most {
            return;
        }
        let mut closed: Vec<(Instant, Source)> = self
            .accounts
            .iter()
            .filter(|(_, account)| account.open == 0)
            .map(|(source, account)| (account.seen, *source))
            .collect();
        let evicted = (most / ADDRESSES_EVICTED_AT_ONCE_DIVISOR)
            .max(1)
            .min(closed.len());
        if evicted == 0 {
            return;
        }
        closed.select_nth_unstable_by_key(evicted - 1, |(seen, _)| *seen);
        for (_, source) in &closed[..evicted] {
            self.accounts.remove(source);
        }
    }

    fn forget_quiet(&mut self, now: Instant) {
        self.accounts.retain(|_, account| {
            account.open > 0 || now.duration_since(account.seen) < FORGET_AFTER
        });
    }
}

impl Admitted {
    pub fn may_make_a_space(&self) -> bool {
        let most = self.guard.allowance.spaces_per_address_per_day;

        self.spending(|account, now| account.spaces.spend(most, now))
    }

    pub fn may_make_a_shared_space(&self) -> bool {
        let most = self.guard.allowance.shared_spaces_per_address_per_day;

        self.spending(|account, now| account.shared_spaces.spend(most, now))
    }

    pub fn may_host_a_room(&self) -> bool {
        let most = self.guard.allowance.rooms_per_address_per_day;

        self.spending(|account, now| account.rooms.spend(most, now))
    }

    fn spending(&self, spend: impl FnOnce(&mut Account, Instant) -> bool) -> bool {
        let now = Instant::now();
        let mut ledger = self.guard.ledger.lock();

        ledger
            .accounts
            .get_mut(&self.source)
            .is_some_and(|account| spend(account, now))
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
            shared_spaces_per_address_per_day: 2,
            rooms_per_address_per_day: 2,
        }
    }

    #[test]
    fn a_shared_space_and_a_room_are_counted_apart_from_a_personal_space() {
        let guard = Guard::new(tight());
        let admitted = guard
            .admit("198.51.100.200".parse().expect("an address"))
            .expect("admitted");

        assert!(admitted.may_make_a_shared_space());
        assert!(admitted.may_make_a_shared_space());
        assert!(!admitted.may_make_a_shared_space());
        assert!(admitted.may_host_a_room());
        assert!(admitted.may_host_a_room());
        assert!(!admitted.may_host_a_room());
        assert!(
            admitted.may_make_a_space(),
            "what a shared space spent came out of a personal space's allowance"
        );
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
    fn a_full_table_lets_go_of_the_address_seen_least_lately_that_holds_nothing_open() {
        let guard = Guard::keeping(tight(), 3);

        let oldest = guard.admit(address(1)).expect("admitted");
        let held = guard.admit(address(2)).expect("admitted");
        let newest = guard.admit(address(3)).expect("admitted");
        drop(oldest);
        drop(newest);

        let arriving = guard.admit(address(4));

        assert!(arriving.is_some());
        let ledger = guard.ledger.lock();
        assert_eq!(ledger.accounts.len(), 3);
        assert!(!ledger.accounts.contains_key(&Source::of(address(1))));
        assert!(ledger.accounts.contains_key(&Source::of(address(2))));
        assert!(ledger.accounts.contains_key(&Source::of(address(3))));
        drop(ledger);
        drop(held);
    }

    #[test]
    fn a_full_table_of_addresses_all_holding_a_connection_turns_a_new_one_away() {
        let guard = Guard::keeping(tight(), 3);

        let held = (1..=3)
            .map(|last| guard.admit(address(last)))
            .collect::<Vec<_>>();

        assert!(held.iter().all(Option::is_some));
        assert!(guard.admit(address(4)).is_none());
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
