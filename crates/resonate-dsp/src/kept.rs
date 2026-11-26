use std::sync::Arc;

pub(crate) const KEPT_DESIGNS_AT_MOST: usize = 4;

pub(crate) struct Kept<Key, Value> {
    entries: Vec<(Key, Arc<Value>)>,
}

impl<Key: PartialEq, Value> Kept<Key, Value> {
    pub(crate) const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub(crate) fn recall(&mut self, wanted: &Key) -> Option<Arc<Value>> {
        let at = self.entries.iter().position(|(held, _)| held == wanted)?;
        let entry = self.entries.remove(at);
        let kept = Arc::clone(&entry.1);

        self.entries.push(entry);
        Some(kept)
    }

    pub(crate) fn keep(&mut self, wanted: Key, value: &Arc<Value>) {
        self.entries.retain(|(held, _)| *held != wanted);
        if self.entries.len() >= KEPT_DESIGNS_AT_MOST {
            self.entries.remove(0);
        }

        self.entries.push((wanted, Arc::clone(value)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_least_recently_recalled_design_is_the_one_let_go() {
        let mut kept = Kept::new();

        for key in 0..KEPT_DESIGNS_AT_MOST as u32 {
            kept.keep(key, &Arc::new(key));
        }

        assert!(kept.recall(&0).is_some());

        kept.keep(100, &Arc::new(100));

        assert!(kept.recall(&0).is_some());
        assert!(kept.recall(&1).is_none());
        assert!(kept.recall(&100).is_some());
    }

    #[test]
    fn keeping_a_key_again_replaces_its_design_and_holds_one_entry() {
        let mut kept = Kept::new();

        kept.keep(7_u8, &Arc::new(1_u32));
        kept.keep(7_u8, &Arc::new(2_u32));

        assert_eq!(kept.entries.len(), 1);
        assert_eq!(kept.recall(&7).as_deref(), Some(&2));
    }
}
