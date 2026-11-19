use crate::candidate::{Candidate, StaticFormat};
use crate::range::{Range, RangeClass};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Evaluation {
    pub range_class: RangeClass,
    pub distance_from_ideal: u32,
    pub square: bool,
    pub source_priority: usize,
    pub format_priority: usize,
    pub short_side: u32,
}

pub fn is_acceptable(candidate: &Candidate, range: &Range) -> bool {
    !matches!(
        candidate.range_class(range),
        RangeClass::BelowMinimum | RangeClass::AboveLadder
    )
}

pub fn evaluate(candidate: &Candidate, range: &Range, format_order: &[StaticFormat]) -> Evaluation {
    let short_side = candidate.short_side();
    Evaluation {
        range_class: candidate.range_class(range),
        distance_from_ideal: short_side.abs_diff(range.ideal),
        square: candidate.is_square(),
        source_priority: candidate.source_priority,
        format_priority: format_order
            .iter()
            .position(|format| *format == candidate.format)
            .unwrap_or(usize::MAX),
        short_side,
    }
}

pub fn compare(
    left: &Candidate,
    right: &Candidate,
    range: &Range,
    format_order: &[StaticFormat],
) -> Ordering {
    let left_eval = evaluate(left, range, format_order);
    let right_eval = evaluate(right, range, format_order);
    (
        !is_acceptable(left, range),
        left_eval.distance_from_ideal,
        !left_eval.square,
        left_eval.source_priority,
        left_eval.format_priority,
        std::cmp::Reverse(left_eval.short_side),
        left.source.as_str(),
    )
        .cmp(&(
            !is_acceptable(right, range),
            right_eval.distance_from_ideal,
            !right_eval.square,
            right_eval.source_priority,
            right_eval.format_priority,
            std::cmp::Reverse(right_eval.short_side),
            right.source.as_str(),
        ))
}

pub fn best_candidate<'a>(
    candidates: &'a [Candidate],
    range: &Range,
    format_order: &[StaticFormat],
) -> Option<&'a Candidate> {
    candidates
        .iter()
        .filter(|candidate| is_acceptable(candidate, range))
        .min_by(|left, right| compare(left, right, range, format_order))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(
        source: &str,
        width: u32,
        height: u32,
        format: StaticFormat,
        source_priority: usize,
    ) -> Candidate {
        Candidate {
            source: source.to_string(),
            width,
            height,
            format,
            source_priority,
        }
    }

    #[test]
    fn below_minimum_is_not_acceptable() {
        let candidate = candidate("a", 1199, 1199, StaticFormat::Jpeg, 0);
        assert!(!is_acceptable(&candidate, &Range::default()));
    }

    #[test]
    fn ladder_boundary_is_acceptable() {
        let candidate = candidate("a", 3600, 3600, StaticFormat::Jpeg, 0);
        assert!(is_acceptable(&candidate, &Range::default()));
    }

    #[test]
    fn above_ladder_is_not_acceptable() {
        let candidate = candidate("a", 3601, 3601, StaticFormat::Jpeg, 0);
        assert!(!is_acceptable(&candidate, &Range::default()));
    }

    #[test]
    fn ideal_beats_higher_resolution() {
        let range = Range::default();
        let ideal = candidate("ideal", 1800, 1800, StaticFormat::Jpeg, 4);
        let large = candidate("large", 2400, 2400, StaticFormat::Jpeg, 0);
        let candidates = vec![large, ideal.clone()];
        let best = best_candidate(&candidates, &range, &[StaticFormat::Jpeg]).unwrap();
        assert_eq!(best, &ideal);
    }

    #[test]
    fn closer_candidate_wins_regardless_of_lower_or_upper_range() {
        let range = Range::default();
        let lower = candidate("lower", 1750, 1750, StaticFormat::Jpeg, 4);
        let upper = candidate("upper", 1900, 1900, StaticFormat::Jpeg, 0);
        let candidates = vec![upper, lower.clone()];
        let best = best_candidate(&candidates, &range, &[StaticFormat::Jpeg]).unwrap();
        assert_eq!(best, &lower);
    }

    #[test]
    fn upper_range_beats_distant_ladder_candidate() {
        let range = Range::default();
        let upper = candidate("upper", 2400, 2400, StaticFormat::Jpeg, 4);
        let ladder = candidate("ladder", 3600, 3600, StaticFormat::Jpeg, 0);
        let candidates = vec![ladder, upper.clone()];
        let best = best_candidate(&candidates, &range, &[StaticFormat::Jpeg]).unwrap();
        assert_eq!(best, &upper);
    }

    #[test]
    fn closer_to_ideal_wins_within_same_range_class() {
        let range = Range::default();
        let near = candidate("near", 1900, 1900, StaticFormat::Jpeg, 4);
        let far = candidate("far", 2300, 2300, StaticFormat::Jpeg, 0);
        let candidates = vec![far, near.clone()];
        let best = best_candidate(&candidates, &range, &[StaticFormat::Jpeg]).unwrap();
        assert_eq!(best, &near);
    }

    #[test]
    fn square_breaks_equal_distance_tie() {
        let range = Range::default();
        let square = candidate("square", 1800, 1800, StaticFormat::Jpeg, 1);
        let non_square = candidate("wide", 2000, 1800, StaticFormat::Jpeg, 0);
        let candidates = vec![non_square, square.clone()];
        let best = best_candidate(&candidates, &range, &[StaticFormat::Jpeg]).unwrap();
        assert_eq!(best, &square);
    }

    #[test]
    fn source_priority_breaks_tie() {
        let range = Range::default();
        let preferred = candidate("preferred", 1800, 1800, StaticFormat::Jpeg, 0);
        let other = candidate("other", 1800, 1800, StaticFormat::Jpeg, 3);
        let candidates = vec![other, preferred.clone()];
        let best = best_candidate(&candidates, &range, &[StaticFormat::Jpeg]).unwrap();
        assert_eq!(best, &preferred);
    }

    #[test]
    fn preferred_format_breaks_tie_after_source_priority() {
        let range = Range::default();
        let jpeg = candidate("same-source-a", 1800, 1800, StaticFormat::Jpeg, 0);
        let png = candidate("same-source-b", 1800, 1800, StaticFormat::Png, 0);
        let candidates = vec![png, jpeg.clone()];
        let best = best_candidate(
            &candidates,
            &range,
            &[StaticFormat::Jpeg, StaticFormat::Png, StaticFormat::Webp],
        )
        .unwrap();
        assert_eq!(best, &jpeg);
    }

    #[test]
    fn invalid_candidates_are_filtered_out() {
        let range = Range::default();
        let too_small = candidate("small", 500, 500, StaticFormat::Jpeg, 0);
        let valid = candidate("valid", 1200, 1200, StaticFormat::Jpeg, 4);
        let candidates = vec![too_small, valid.clone()];
        let best = best_candidate(&candidates, &range, &[StaticFormat::Jpeg]).unwrap();
        assert_eq!(best, &valid);
    }

    #[test]
    fn no_valid_candidates_returns_none() {
        let range = Range::default();
        let candidates = vec![
            candidate("small", 500, 500, StaticFormat::Jpeg, 0),
            candidate("huge", 10000, 10000, StaticFormat::Jpeg, 0),
        ];
        assert_eq!(
            best_candidate(&candidates, &range, &[StaticFormat::Jpeg]),
            None
        );
    }
}
