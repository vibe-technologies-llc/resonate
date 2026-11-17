pub(crate) struct LastTold {
    owner: Option<u64>,
}

impl LastTold {
    pub(crate) const fn new() -> Self {
        Self { owner: None }
    }

    pub(crate) const fn told_by(&mut self, owner: u64) {
        self.owner = Some(owner);
    }

    pub(crate) fn let_go_by(&mut self, owner: u64) -> bool {
        let told_it_last = self.owner == Some(owner);
        if told_it_last {
            self.owner = None;
        }
        told_it_last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_owner_that_told_last_lets_what_it_told_go() {
        let mut told = LastTold::new();
        told.told_by(1);

        let let_go = told.let_go_by(1);
        let again = told.let_go_by(1);

        assert!(let_go);
        assert!(!again, "what was let go was let go twice");
    }

    #[test]
    fn an_owner_dropped_after_another_told_since_leaves_what_the_other_told() {
        let mut told = LastTold::new();
        told.told_by(1);
        told.told_by(2);

        let first = told.let_go_by(1);
        let second = told.let_go_by(2);

        assert!(!first, "the first owner cleared what the second had told");
        assert!(second);
    }

    #[test]
    fn an_owner_that_never_told_lets_nothing_go() {
        let mut told = LastTold::new();

        assert!(!told.let_go_by(9));
    }
}
