use std::collections::BTreeMap;

pub(crate) struct Targets<T> {
    owned: BTreeMap<u64, Vec<T>>,
}

impl<T> Targets<T> {
    pub(crate) const fn new() -> Self {
        Self {
            owned: BTreeMap::new(),
        }
    }

    pub(crate) fn add(&mut self, owner: u64, target: T) {
        self.owned.entry(owner).or_default().push(target);
    }

    pub(crate) fn take(&mut self, owner: u64) -> Vec<T> {
        self.owned.remove(&owner).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taking_one_owners_targets_leaves_every_other_owners_in_place() {
        let mut targets = Targets::new();
        targets.add(1, "play");
        targets.add(1, "pause");
        targets.add(2, "play");

        let first = targets.take(1);
        let again = targets.take(1);
        let second = targets.take(2);

        assert_eq!(first, ["play", "pause"]);
        assert!(again.is_empty());
        assert_eq!(second, ["play"]);
    }

    #[test]
    fn an_owner_that_added_nothing_takes_nothing() {
        let mut targets = Targets::<u8>::new();

        assert!(targets.take(9).is_empty());
    }
}
