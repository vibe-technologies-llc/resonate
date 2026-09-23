use std::{collections::BTreeMap, hash::Hash, num::NonZeroUsize};

use ahash::AHashMap;

struct Held<V> {
    value: V,
    used: u64,
}

pub(crate) struct Recent<K, V> {
    held: AHashMap<K, Held<V>>,
    order: BTreeMap<u64, K>,
    limit: NonZeroUsize,
    clock: u64,
}

impl<K: Clone + Eq + Hash, V> Recent<K, V> {
    pub(crate) fn new(limit: NonZeroUsize) -> Self {
        Self {
            held: AHashMap::with_capacity(limit.get()),
            order: BTreeMap::new(),
            limit,
            clock: 0,
        }
    }

    pub(crate) fn holds(&self, key: &K) -> bool {
        self.held.contains_key(key)
    }

    pub(crate) fn get(&mut self, key: &K) -> Option<&V> {
        let now = self.tick();
        let held = self.held.get_mut(key)?;
        self.order.remove(&held.used);
        held.used = now;
        self.order.insert(now, key.clone());
        Some(&held.value)
    }

    pub(crate) fn insert(&mut self, key: K, value: V) {
        let now = self.tick();
        match self.held.get(&key) {
            Some(held) => {
                self.order.remove(&held.used);
            }
            None => self.evict_until_one_fits(),
        }
        self.order.insert(now, key.clone());
        self.held.insert(key, Held { value, used: now });
    }

    fn evict_until_one_fits(&mut self) {
        while self.held.len() >= self.limit.get() {
            let Some((_, stale)) = self.order.pop_first() else {
                return;
            };
            self.held.remove(&stale);
        }
    }

    fn tick(&mut self) -> u64 {
        self.clock = self.clock.saturating_add(1);
        self.clock
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bound(n: usize) -> NonZeroUsize {
        NonZeroUsize::new(n).expect("a non-zero bound")
    }

    fn filled(limit: usize, keys: &[u32]) -> Recent<u32, u32> {
        let mut cache = Recent::new(bound(limit));
        for key in keys {
            cache.insert(*key, key * 10);
        }
        cache
    }

    #[test]
    fn a_held_value_comes_back_until_it_is_evicted() {
        let mut cache = filled(4, &[1, 2, 3]);

        assert_eq!(cache.get(&2), Some(&20));
        assert_eq!(cache.get(&9), None);
        assert_eq!(cache.get(&1), Some(&10));
    }

    #[test]
    fn the_entry_nothing_has_looked_at_is_the_one_that_goes() {
        let mut cache = filled(3, &[1, 2, 3]);
        assert_eq!(cache.get(&1), Some(&10));

        cache.insert(4, 40);

        assert_eq!(cache.get(&2), None, "the entry in use was evicted instead");
        assert_eq!(cache.get(&1), Some(&10));
        assert_eq!(cache.get(&3), Some(&30));
        assert_eq!(cache.get(&4), Some(&40));
    }

    #[test]
    fn a_scroll_longer_than_the_bound_keeps_what_it_just_read() {
        let mut cache = filled(8, &(0..64).collect::<Vec<_>>());

        for key in 0..56 {
            assert_eq!(cache.get(&key), None, "row {key} outstayed the bound");
        }
        for key in 56..64 {
            assert_eq!(cache.get(&key), Some(&(key * 10)), "row {key} was dropped");
        }
    }

    #[test]
    fn writing_over_a_held_key_evicts_nothing() {
        let mut cache = filled(3, &[1, 2, 3]);
        cache.insert(2, 99);

        assert_eq!(
            cache.get(&1),
            Some(&10),
            "writing over a key evicted another"
        );
        assert_eq!(cache.get(&2), Some(&99));
        assert_eq!(cache.get(&3), Some(&30));
    }

    #[test]
    fn the_order_keeps_one_entry_for_every_key_held() {
        let mut cache = filled(8, &(0..64).collect::<Vec<_>>());
        for key in 0..64 {
            cache.get(&key);
        }
        for key in 0..64 {
            cache.insert(key, key * 100);
        }

        assert_eq!(cache.held.len(), 8);
        assert_eq!(
            cache.order.len(),
            cache.held.len(),
            "the use order and the entries it evicts by fell out of step"
        );
    }

    #[test]
    fn a_bound_of_one_holds_only_what_it_last_saw() {
        let mut cache = filled(1, &[1, 2]);

        assert_eq!(cache.get(&1), None);
        assert_eq!(cache.get(&2), Some(&20));
    }
}
