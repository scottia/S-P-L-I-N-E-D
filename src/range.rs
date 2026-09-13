#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Range {
    pub min: u32,
    pub ideal: u32,
    pub max: u32,
    pub ladder: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeClass {
    BelowMinimum,
    LowerRange,
    Ideal,
    UpperRange,
    Ladder,
    AboveLadder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeError {
    MinNotBelowIdeal,
    IdealAboveMax,
    MaxNotBelowLadder,
}

impl Default for Range {
    fn default() -> Self {
        Self {
            min: 1200,
            ideal: 1800,
            max: 2400,
            ladder: 3600,
        }
    }
}

impl Range {
    pub fn validate(&self) -> Result<(), RangeError> {
        if self.min >= self.ideal {
            return Err(RangeError::MinNotBelowIdeal);
        }

        if self.ideal > self.max {
            return Err(RangeError::IdealAboveMax);
        }

        if self.max >= self.ladder {
            return Err(RangeError::MaxNotBelowLadder);
        }

        Ok(())
    }

    pub fn classify(&self, pixels: u32) -> RangeClass {
        if pixels < self.min {
            RangeClass::BelowMinimum
        } else if pixels < self.ideal {
            RangeClass::LowerRange
        } else if pixels == self.ideal {
            RangeClass::Ideal
        } else if pixels <= self.max {
            RangeClass::UpperRange
        } else if pixels <= self.ladder {
            RangeClass::Ladder
        } else {
            RangeClass::AboveLadder
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range() -> Range {
        Range::default()
    }

    #[test]
    fn default_values_are_correct() {
        let range = range();

        assert_eq!(range.min, 1200);
        assert_eq!(range.ideal, 1800);
        assert_eq!(range.max, 2400);
        assert_eq!(range.ladder, 3600);
    }

    #[test]
    fn below_minimum_is_rejected_class() {
        assert_eq!(range().classify(1199), RangeClass::BelowMinimum);
    }

    #[test]
    fn minimum_enters_lower_range() {
        assert_eq!(range().classify(1200), RangeClass::LowerRange);
    }

    #[test]
    fn pixel_below_ideal_is_lower_range() {
        assert_eq!(range().classify(1799), RangeClass::LowerRange);
    }

    #[test]
    fn ideal_is_exact() {
        assert_eq!(range().classify(1800), RangeClass::Ideal);
    }

    #[test]
    fn pixel_above_ideal_is_upper_range() {
        assert_eq!(range().classify(1801), RangeClass::UpperRange);
    }

    #[test]
    fn maximum_is_still_upper_range() {
        assert_eq!(range().classify(2400), RangeClass::UpperRange);
    }

    #[test]
    fn pixel_above_maximum_enters_ladder() {
        assert_eq!(range().classify(2401), RangeClass::Ladder);
    }

    #[test]
    fn ladder_boundary_is_accepted_ladder_class() {
        assert_eq!(range().classify(3600), RangeClass::Ladder);
    }

    #[test]
    fn pixel_above_ladder_is_rejected_class() {
        assert_eq!(range().classify(3601), RangeClass::AboveLadder);
    }

    #[test]
    fn default_range_is_valid() {
        assert_eq!(range().validate(), Ok(()));
    }

    #[test]
    fn minimum_must_be_below_ideal() {
        let range = Range {
            min: 1800,
            ideal: 1800,
            max: 2400,
            ladder: 3600,
        };

        assert_eq!(range.validate(), Err(RangeError::MinNotBelowIdeal));
    }

    #[test]
    fn ideal_may_equal_maximum() {
        let range = Range {
            min: 1200,
            ideal: 2400,
            max: 2400,
            ladder: 3600,
        };

        assert_eq!(range.validate(), Ok(()));
    }

    #[test]
    fn ideal_cannot_exceed_maximum() {
        let range = Range {
            min: 1200,
            ideal: 2500,
            max: 2400,
            ladder: 3600,
        };

        assert_eq!(range.validate(), Err(RangeError::IdealAboveMax));
    }

    #[test]
    fn maximum_must_be_below_ladder() {
        let range = Range {
            min: 1200,
            ideal: 1800,
            max: 3600,
            ladder: 3600,
        };

        assert_eq!(range.validate(), Err(RangeError::MaxNotBelowLadder));
    }
}
