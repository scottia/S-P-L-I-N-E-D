use crate::candidate::{Candidate, StaticFormat};
use crate::evaluate::compare;
use crate::range::{Range, RangeClass};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MinimumRangeType {
    #[serde(rename = "BelowMinimum", alias = "below_minimum")]
    BelowMinimum,
    #[default]
    #[serde(rename = "LowerRange", alias = "lower_range")]
    LowerRange,
    #[serde(rename = "Ideal", alias = "ideal")]
    Ideal,
    #[serde(rename = "UpperRange", alias = "upper_range")]
    UpperRange,
    #[serde(rename = "Ladder", alias = "ladder")]
    Ladder,
    #[serde(rename = "AboveLadder", alias = "above_ladder")]
    AboveLadder,
}

impl MinimumRangeType {
    pub fn rank(self) -> u8 {
        match self {
            Self::BelowMinimum => 0,
            Self::LowerRange => 1,
            Self::Ideal => 2,
            Self::UpperRange => 3,
            Self::Ladder => 4,
            Self::AboveLadder => 5,
        }
    }

    pub fn derived_minimum(self, range: &Range) -> u32 {
        match self {
            Self::BelowMinimum => 0,
            Self::LowerRange => range.min,
            Self::Ideal => range.ideal,
            Self::UpperRange => range.ideal.saturating_add(1),
            Self::Ladder => range.max.saturating_add(1),
            Self::AboveLadder => range.ladder.saturating_add(1),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SourcePolicyConfig {
    pub enabled: bool,
    pub source_override: bool,
    pub minimum_range_type: MinimumRangeType,
    pub allow_below_minimum_fallback: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_short_side: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_short_side: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum_height: Option<u32>,
    pub primary_image_only: bool,
}

impl Default for SourcePolicyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            source_override: false,
            minimum_range_type: MinimumRangeType::LowerRange,
            allow_below_minimum_fallback: false,
            minimum_short_side: None,
            maximum_short_side: None,
            minimum_width: None,
            minimum_height: None,
            primary_image_only: true,
        }
    }
}

impl SourcePolicyConfig {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("minimum_short_side", self.minimum_short_side),
            ("maximum_short_side", self.maximum_short_side),
            ("minimum_width", self.minimum_width),
            ("minimum_height", self.minimum_height),
        ] {
            if value == Some(0) {
                return Err(format!("{name} must be greater than zero when configured"));
            }
        }

        if let (Some(minimum), Some(maximum)) = (self.minimum_short_side, self.maximum_short_side)
            && minimum > maximum
        {
            return Err("minimum_short_side cannot exceed maximum_short_side".to_string());
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourcePolicyStatus {
    Accept,
    Fallback,
    Reject,
}

impl SourcePolicyStatus {
    fn priority(self) -> u8 {
        match self {
            Self::Accept => 0,
            Self::Fallback => 1,
            Self::Reject => 2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePolicyDecision {
    pub status: SourcePolicyStatus,
    pub reason: String,
}

pub fn active_policy<'a>(
    policies: &'a BTreeMap<String, SourcePolicyConfig>,
    source: &str,
) -> Option<&'a SourcePolicyConfig> {
    policies
        .get(&source.to_ascii_lowercase())
        .filter(|policy| policy.source_override)
}

pub fn reference_allowed(
    policies: &BTreeMap<String, SourcePolicyConfig>,
    source: &str,
    front: bool,
) -> bool {
    match active_policy(policies, source) {
        Some(policy) => !policy.primary_image_only || front,
        None => front,
    }
}

pub fn global_range_decision(width: u32, height: u32, range: &Range) -> SourcePolicyDecision {
    let short_side = width.min(height);
    let range_class = range.classify(short_side);
    let accepted = !matches!(
        range_class,
        RangeClass::BelowMinimum | RangeClass::AboveLadder
    );

    SourcePolicyDecision {
        status: if accepted {
            SourcePolicyStatus::Accept
        } else {
            SourcePolicyStatus::Reject
        },
        reason: if accepted {
            "Accepted by the global artwork range".to_string()
        } else if range_class == RangeClass::BelowMinimum {
            format!(
                "Short side {short_side}px is below global minimum {}px",
                range.min
            )
        } else {
            format!(
                "Short side {short_side}px is above global ladder {}px",
                range.ladder
            )
        },
    }
}

pub fn source_override_decision(
    policy: &SourcePolicyConfig,
    width: u32,
    height: u32,
    range: &Range,
) -> SourcePolicyDecision {
    let short_side = width.min(height);

    if let Some(minimum) = policy.minimum_short_side
        && short_side < minimum
    {
        return SourcePolicyDecision {
            status: SourcePolicyStatus::Reject,
            reason: format!("Short side {short_side}px is below source minimum {minimum}px"),
        };
    }

    if let Some(maximum) = policy.maximum_short_side
        && short_side > maximum
    {
        return SourcePolicyDecision {
            status: SourcePolicyStatus::Reject,
            reason: format!("Short side {short_side}px is above source maximum {maximum}px"),
        };
    }

    if let Some(minimum) = policy.minimum_width
        && width < minimum
    {
        return SourcePolicyDecision {
            status: SourcePolicyStatus::Reject,
            reason: format!("Width {width}px is below source minimum {minimum}px"),
        };
    }

    if let Some(minimum) = policy.minimum_height
        && height < minimum
    {
        return SourcePolicyDecision {
            status: SourcePolicyStatus::Reject,
            reason: format!("Height {height}px is below source minimum {minimum}px"),
        };
    }

    let range_class = range.classify(short_side);
    let range_rank = range_class_rank(range_class);
    if range_rank < policy.minimum_range_type.rank() {
        let adjacent_fallback_rank = policy.minimum_range_type.rank().checked_sub(1);
        if policy.allow_below_minimum_fallback && adjacent_fallback_rank == Some(range_rank) {
            return SourcePolicyDecision {
                status: SourcePolicyStatus::Fallback,
                reason: format!(
                    "Candidate range {range_class:?} is the single fallback level below source minimum {:?}",
                    policy.minimum_range_type
                ),
            };
        }

        return SourcePolicyDecision {
            status: SourcePolicyStatus::Reject,
            reason: format!(
                "Candidate range {range_class:?} is below source minimum {:?}",
                policy.minimum_range_type
            ),
        };
    }

    SourcePolicyDecision {
        status: SourcePolicyStatus::Accept,
        reason: format!(
            "Candidate range {range_class:?} meets source minimum {:?}",
            policy.minimum_range_type
        ),
    }
}

pub fn effective_candidate_decision(
    candidate: &Candidate,
    range: &Range,
    policies: &BTreeMap<String, SourcePolicyConfig>,
) -> SourcePolicyDecision {
    match active_policy(policies, &candidate.source) {
        Some(policy) => source_override_decision(policy, candidate.width, candidate.height, range),
        None => global_range_decision(candidate.width, candidate.height, range),
    }
}

pub fn best_candidate_index(
    candidates: &[Candidate],
    range: &Range,
    format_order: &[StaticFormat],
    policies: &BTreeMap<String, SourcePolicyConfig>,
) -> Option<usize> {
    candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            let status = effective_candidate_decision(candidate, range, policies).status;
            (status != SourcePolicyStatus::Reject).then_some((index, candidate, status))
        })
        .min_by(|left, right| {
            left.2
                .priority()
                .cmp(&right.2.priority())
                .then_with(|| compare(left.1, right.1, range, format_order))
        })
        .map(|(index, _, _)| index)
}

fn range_class_rank(range_class: RangeClass) -> u8 {
    match range_class {
        RangeClass::BelowMinimum => 0,
        RangeClass::LowerRange => 1,
        RangeClass::Ideal => 2,
        RangeClass::UpperRange => 3,
        RangeClass::Ladder => 4,
        RangeClass::AboveLadder => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> SourcePolicyConfig {
        SourcePolicyConfig {
            source_override: true,
            ..SourcePolicyConfig::default()
        }
    }

    #[test]
    fn override_off_preserves_global_range_behavior() {
        let policies = BTreeMap::new();
        let small = Candidate {
            source: "discogs".to_string(),
            width: 600,
            height: 600,
            format: StaticFormat::Jpeg,
            source_priority: 0,
        };
        assert_eq!(
            effective_candidate_decision(&small, &Range::default(), &policies).status,
            SourcePolicyStatus::Reject
        );
    }

    #[test]
    fn lower_range_override_accepts_above_ladder() {
        let decision = source_override_decision(&policy(), 5000, 5000, &Range::default());
        assert_eq!(decision.status, SourcePolicyStatus::Accept);
    }

    #[test]
    fn adjacent_lower_range_can_be_fallback_only() {
        let mut configured = policy();
        configured.minimum_range_type = MinimumRangeType::Ideal;
        configured.allow_below_minimum_fallback = true;
        assert_eq!(
            source_override_decision(&configured, 1500, 1500, &Range::default()).status,
            SourcePolicyStatus::Fallback
        );
        assert_eq!(
            source_override_decision(&configured, 600, 600, &Range::default()).status,
            SourcePolicyStatus::Reject
        );
    }

    #[test]
    fn lower_range_uses_below_minimum_as_its_adjacent_fallback() {
        let mut configured = policy();
        configured.allow_below_minimum_fallback = true;
        assert_eq!(
            source_override_decision(&configured, 600, 600, &Range::default()).status,
            SourcePolicyStatus::Fallback
        );
    }

    #[test]
    fn explicit_minimum_splits_lower_range() {
        let mut configured = policy();
        configured.minimum_short_side = Some(1500);
        assert_eq!(
            source_override_decision(&configured, 1499, 1700, &Range::default()).status,
            SourcePolicyStatus::Reject
        );
        assert_eq!(
            source_override_decision(&configured, 1500, 1700, &Range::default()).status,
            SourcePolicyStatus::Accept
        );
    }

    #[test]
    fn primary_image_filter_changes_only_for_active_override() {
        let mut policies = BTreeMap::new();
        policies.insert("coverartarchive".to_string(), policy());
        assert!(!reference_allowed(&policies, "coverartarchive", false));
        policies
            .get_mut("coverartarchive")
            .unwrap()
            .primary_image_only = false;
        assert!(reference_allowed(&policies, "coverartarchive", false));
    }
}
