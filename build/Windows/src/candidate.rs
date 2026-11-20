use crate::inspect::inspect_image;
use crate::range::{Range, RangeClass};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StaticFormat {
    Jpeg,
    Png,
    Webp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub source: String,
    pub width: u32,
    pub height: u32,
    pub format: StaticFormat,
    pub source_priority: usize,
}

impl Candidate {
    pub fn from_file(
        source: impl Into<String>,
        path: &Path,
        source_priority: usize,
    ) -> Result<Self, String> {
        let inspected = inspect_image(path)?;

        Ok(Self {
            source: source.into(),
            width: inspected.width,
            height: inspected.height,
            format: inspected.format,
            source_priority,
        })
    }

    pub fn short_side(&self) -> u32 {
        self.width.min(self.height)
    }

    pub fn long_side(&self) -> u32 {
        self.width.max(self.height)
    }

    pub fn is_square(&self) -> bool {
        self.width == self.height
    }

    pub fn range_class(&self, range: &Range) -> RangeClass {
        range.classify(self.short_side())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_side_uses_smaller_dimension() {
        let candidate = Candidate {
            source: "test".to_string(),
            width: 2400,
            height: 1800,
            format: StaticFormat::Jpeg,
            source_priority: 0,
        };

        assert_eq!(candidate.short_side(), 1800);
        assert_eq!(candidate.long_side(), 2400);
    }

    #[test]
    fn square_candidate_is_detected() {
        let candidate = Candidate {
            source: "test".to_string(),
            width: 1800,
            height: 1800,
            format: StaticFormat::Png,
            source_priority: 0,
        };

        assert!(candidate.is_square());
    }

    #[test]
    fn non_square_candidate_is_detected() {
        let candidate = Candidate {
            source: "test".to_string(),
            width: 2400,
            height: 1800,
            format: StaticFormat::Webp,
            source_priority: 0,
        };

        assert!(!candidate.is_square());
    }

    #[test]
    fn classification_uses_short_side() {
        let candidate = Candidate {
            source: "test".to_string(),
            width: 2400,
            height: 900,
            format: StaticFormat::Jpeg,
            source_priority: 0,
        };

        assert_eq!(
            candidate.range_class(&Range::default()),
            RangeClass::BelowMinimum
        );
    }
}
