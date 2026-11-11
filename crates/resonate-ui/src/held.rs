use std::{fmt, ops::Index, sync::Arc};

pub struct Rows<T> {
    chunks: Arc<[Arc<[T]>]>,
    starts: Arc<[usize]>,
    len: usize,
}

impl<T> Rows<T> {
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn get(&self, at: usize) -> Option<&T> {
        if at >= self.len {
            return None;
        }
        let chunk = self.starts.partition_point(|start| *start <= at) - 1;
        self.chunks[chunk].get(at - self.starts[chunk])
    }

    pub fn first(&self) -> Option<&T> {
        self.chunks.first().and_then(|chunk| chunk.first())
    }

    pub fn last(&self) -> Option<&T> {
        self.chunks.last().and_then(|chunk| chunk.last())
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &T> + Clone {
        self.chunks.iter().flat_map(|chunk| chunk.iter())
    }

    pub fn chunks(&self) -> impl Iterator<Item = &[T]> {
        self.chunks.iter().map(|chunk| &**chunk)
    }

    pub fn same_as(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.chunks, &other.chunks)
    }

    pub fn identity(&self) -> *const () {
        Arc::as_ptr(&self.chunks).cast::<()>()
    }

    pub(crate) fn appended(&self, more: Vec<T>) -> Self {
        if more.is_empty() {
            return self.clone();
        }
        let added = more.len();
        let chunks: Arc<[Arc<[T]>]> = self
            .chunks
            .iter()
            .cloned()
            .chain([Arc::from(more)])
            .collect();
        let starts: Arc<[usize]> = self.starts.iter().copied().chain([self.len]).collect();

        Self {
            chunks,
            starts,
            len: self.len + added,
        }
    }
}

impl<T: Clone> Rows<T> {
    pub fn to_vec(&self) -> Vec<T> {
        self.iter().cloned().collect()
    }

    pub fn whole(&self) -> Arc<[T]> {
        match &*self.chunks {
            [] => Arc::from(Vec::new()),
            [only] => Arc::clone(only),
            _ => self.iter().cloned().collect(),
        }
    }
}

impl<T> Clone for Rows<T> {
    fn clone(&self) -> Self {
        Self {
            chunks: Arc::clone(&self.chunks),
            starts: Arc::clone(&self.starts),
            len: self.len,
        }
    }
}

impl<T> Default for Rows<T> {
    fn default() -> Self {
        Self {
            chunks: Arc::from(Vec::new()),
            starts: Arc::from(Vec::new()),
            len: 0,
        }
    }
}

impl<T> From<Arc<[T]>> for Rows<T> {
    fn from(rows: Arc<[T]>) -> Self {
        if rows.is_empty() {
            return Self::default();
        }
        let len = rows.len();

        Self {
            chunks: Arc::from([rows]),
            starts: Arc::from([0]),
            len,
        }
    }
}

impl<T> From<Vec<T>> for Rows<T> {
    fn from(rows: Vec<T>) -> Self {
        Self::default().appended(rows)
    }
}

impl<T> Index<usize> for Rows<T> {
    type Output = T;

    fn index(&self, at: usize) -> &T {
        self.get(at)
            .unwrap_or_else(|| panic!("row {at} is past the {} rows held", self.len))
    }
}

impl<T: PartialEq> PartialEq<[T]> for Rows<T> {
    fn eq(&self, other: &[T]) -> bool {
        self.len == other.len() && self.iter().eq(other.iter())
    }
}

impl<T: PartialEq> PartialEq for Rows<T> {
    fn eq(&self, other: &Self) -> bool {
        self.same_as(other) || (self.len == other.len && self.iter().eq(other.iter()))
    }
}

impl<T: fmt::Debug> fmt::Debug for Rows<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}

impl<'a, T> IntoIterator for &'a Rows<T> {
    type Item = &'a T;
    type IntoIter = Box<dyn DoubleEndedIterator<Item = &'a T> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::Rows;

    #[test]
    fn rows_appended_share_what_was_held_and_read_as_one_run() {
        let held = Rows::from(vec![0, 1, 2]);
        let grown = held
            .appended(vec![3, 4])
            .appended(Vec::new())
            .appended(vec![5]);

        assert_eq!(grown.len(), 6);
        assert_eq!(
            grown.iter().copied().collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5]
        );
        assert_eq!(
            grown.iter().rev().copied().collect::<Vec<_>>(),
            [5, 4, 3, 2, 1, 0]
        );
        assert_eq!(
            (0..6).map(|at| grown[at]).collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5]
        );
        assert_eq!(grown.get(6), None);
        assert_eq!((grown.first(), grown.last()), (Some(&0), Some(&5)));
        assert!(Arc::ptr_eq(&held.chunks[0], &grown.chunks[0]));
        assert_eq!(held.len(), 3);
        assert!(!held.same_as(&grown));
        assert!(grown.same_as(&grown.clone()));
        assert_eq!(grown.whole().as_ref(), [0, 1, 2, 3, 4, 5]);
        assert_eq!(Rows::<u8>::default().first(), None);
        assert!(Rows::<u8>::from(Vec::new()).is_empty());
    }
}
