//! A fixed-size history of numbers for sparklines (`docs/design/octopit-ux.md` §5).

use std::collections::VecDeque;

/// The last `capacity` samples of one value, oldest first.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    capacity: usize,
    samples: VecDeque<f64>,
}

impl Series {
    /// An empty series that keeps at most `capacity` samples (at least 1).
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            capacity,
            samples: VecDeque::with_capacity(capacity),
        }
    }

    /// Adds a sample; the oldest one drops out when full. Non-finite values are ignored.
    pub fn push(&mut self, value: f64) {
        if !value.is_finite() {
            return;
        }
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(value);
    }

    /// Number of samples held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// Whether nothing was added yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The newest sample.
    #[must_use]
    pub fn last(&self) -> Option<f64> {
        self.samples.back().copied()
    }

    /// The samples, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = f64> + '_ {
        self.samples.iter().copied()
    }

    /// Smallest and largest sample.
    #[must_use]
    pub fn range(&self) -> Option<(f64, f64)> {
        let mut it = self.iter();
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), v| (lo.min(v), hi.max(v))))
    }

    /// The samples scaled to `0.0..=1.0` between `floor` and `ceil` (a flat line maps to 0).
    #[must_use]
    pub fn normalized(&self, floor: f64, ceil: f64) -> Vec<f64> {
        let span = ceil - floor;
        self.iter()
            .map(|v| {
                if span > 0.0 {
                    ((v - floor) / span).clamp(0.0, 1.0)
                } else {
                    0.0
                }
            })
            .collect()
    }

    /// Mean of the samples.
    #[must_use]
    pub fn mean(&self) -> Option<f64> {
        if self.samples.is_empty() {
            return None;
        }
        #[allow(clippy::cast_precision_loss)]
        Some(self.samples.iter().sum::<f64>() / self.samples.len() as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_the_newest_samples() {
        let mut s = Series::new(3);
        for v in 1..=5 {
            s.push(f64::from(v));
        }
        assert_eq!(s.iter().collect::<Vec<_>>(), vec![3.0, 4.0, 5.0]);
        assert_eq!(s.len(), 3);
        assert_eq!(s.last(), Some(5.0));
        assert_eq!(s.range(), Some((3.0, 5.0)));
        assert_eq!(s.mean(), Some(4.0));
    }

    #[test]
    fn bad_values_and_zero_capacity_are_safe() {
        let mut s = Series::new(0);
        s.push(f64::NAN);
        s.push(f64::INFINITY);
        assert!(s.is_empty());
        s.push(1.0);
        s.push(2.0);
        assert_eq!(s.len(), 1);
        assert_eq!(Series::new(4).range(), None);
        assert_eq!(Series::new(4).mean(), None);
    }

    #[test]
    fn normalizing_clamps_and_handles_a_flat_range() {
        let mut s = Series::new(4);
        s.push(0.0);
        s.push(50.0);
        s.push(150.0);
        assert_eq!(s.normalized(0.0, 100.0), vec![0.0, 0.5, 1.0]);
        assert_eq!(s.normalized(5.0, 5.0), vec![0.0, 0.0, 0.0]);
    }
}
