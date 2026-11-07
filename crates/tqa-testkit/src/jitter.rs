//! Predictable jitter fractions for retry-policy tests.

use std::io;
use tqa_llm::Jitter;

/// Repeats one validated value or a supplied finite sequence.
#[derive(Clone, Debug)]
pub struct FixedJitter {
    values: Vec<f64>,
    index: usize,
}

impl FixedJitter {
    /// Uses a single fraction for every request.
    ///
    /// # Errors
    /// Rejects non-finite fractions or values outside [0, 1).
    pub fn new(value: f64) -> io::Result<Self> {
        Self::sequence(vec![value])
    }
    /// Cycles through a nonempty sequence in its original order.
    ///
    /// # Errors
    /// Rejects an empty sequence or a value outside [0, 1).
    pub fn sequence(values: Vec<f64>) -> io::Result<Self> {
        if values.is_empty() || values.iter().any(|value| !(0.0..1.0).contains(value)) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "jitter fractions must lie within [0, 1)",
            ));
        }
        Ok(Self { values, index: 0 })
    }
}

impl Jitter for FixedJitter {
    fn next_fraction(&mut self) -> f64 {
        let value = self.values[self.index];
        self.index = (self.index + 1) % self.values.len();
        value
    }
}
