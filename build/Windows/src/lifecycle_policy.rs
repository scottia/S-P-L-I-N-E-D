//! Fail-closed reader for the shared Windows/WebGUI lifecycle contract.
//!
//! The JSON is embedded at build time and validated once per process. Reading
//! it performs no database, network, provider, catalog, or persistence work.

use serde::Deserialize;
use std::collections::BTreeSet;
use std::sync::LazyLock;

const DEFAULT_POLICY: &str = include_str!("../../../schema/lifecycle-policy.json");
const POLICY_ID: &str = "splined-shared-lifecycle-v1";
const REQUIREMENT_IDS: [&str; 9] = [
    "LIFE-001", "LIFE-002", "LIFE-003", "LIFE-004", "LIFE-005", "LIFE-006", "LIFE-007", "LIFE-008",
    "LIFE-009",
];

#[derive(Debug, Clone, Deserialize)]
pub struct LifecyclePolicy {
    pub schema_version: u32,
    pub policy_id: String,
    pub runtimes: Vec<String>,
    pub application_scope: std::collections::BTreeMap<String, String>,
    pub requirements: Vec<LifecycleRequirement>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LifecycleRequirement {
    pub id: String,
    pub applies_to: Vec<String>,
    pub webgui_refinement: String,
    pub title: String,
    pub rule: String,
    pub acceptance: Vec<String>,
}

static POLICY: LazyLock<LifecyclePolicy> = LazyLock::new(|| {
    parse_policy(DEFAULT_POLICY).expect("repository lifecycle policy must be valid")
});

pub fn policy() -> &'static LifecyclePolicy {
    &POLICY
}

pub fn requirement(requirement_id: &str) -> Option<&'static LifecycleRequirement> {
    policy()
        .requirements
        .iter()
        .find(|requirement| requirement.id == requirement_id)
}

pub fn validate_policy(text: &str) -> Result<(), String> {
    parse_policy(text).map(|_| ())
}

fn parse_policy(text: &str) -> Result<LifecyclePolicy, String> {
    let value = serde_json::from_str::<LifecyclePolicy>(text)
        .map_err(|error| format!("invalid lifecycle policy JSON: {error}"))?;
    if value.schema_version != 1 {
        return Err(format!(
            "unsupported lifecycle policy schema {}",
            value.schema_version
        ));
    }
    if value.policy_id != POLICY_ID {
        return Err("unexpected lifecycle policy_id".to_string());
    }
    let expected_runtimes = BTreeSet::from(["windows", "webgui"]);
    let runtimes = value
        .runtimes
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if runtimes != expected_runtimes {
        return Err("lifecycle policy must apply to Windows and WebGUI".to_string());
    }
    if value
        .application_scope
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != expected_runtimes
        || value
            .application_scope
            .values()
            .any(|label| label.trim().is_empty())
    {
        return Err("lifecycle policy application_scope is invalid".to_string());
    }
    if value.requirements.len() != REQUIREMENT_IDS.len() {
        return Err("lifecycle policy requires ordered LIFE-001..009".to_string());
    }
    for (requirement, expected_id) in value.requirements.iter().zip(REQUIREMENT_IDS) {
        if requirement.id != expected_id {
            return Err("lifecycle policy requires ordered LIFE-001..009".to_string());
        }
        let applies_to = requirement
            .applies_to
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if applies_to != expected_runtimes {
            return Err(format!(
                "{} must apply to Windows and WebGUI",
                requirement.id
            ));
        }
        if requirement.webgui_refinement != requirement.id.replace("LIFE-", "WEB-LIFE-") {
            return Err(format!(
                "{} has an invalid WebGUI refinement",
                requirement.id
            ));
        }
        if requirement.title.trim().is_empty() || requirement.rule.trim().is_empty() {
            return Err(format!("{} title and rule are required", requirement.id));
        }
        if requirement.acceptance.len() != 3
            || requirement
                .acceptance
                .iter()
                .any(|item| item.trim().is_empty())
        {
            return Err(format!(
                "{} requires three acceptance statements",
                requirement.id
            ));
        }
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_policy_loads_all_shared_requirements() {
        assert_eq!(policy().requirements.len(), 9);
        for id in REQUIREMENT_IDS {
            assert_eq!(requirement(id).map(|rule| rule.id.as_str()), Some(id));
        }
    }

    #[test]
    fn invalid_identity_scope_order_and_refinement_fail_closed() {
        let canonical: serde_json::Value =
            serde_json::from_str(DEFAULT_POLICY).expect("canonical lifecycle JSON");
        for changed in [
            {
                let mut value = canonical.clone();
                value["policy_id"] = serde_json::json!("wrong");
                value
            },
            {
                let mut value = canonical.clone();
                value["runtimes"] = serde_json::json!(["windows"]);
                value
            },
            {
                let mut value = canonical.clone();
                value["requirements"].as_array_mut().unwrap().swap(0, 1);
                value
            },
            {
                let mut value = canonical.clone();
                value["requirements"][0]["webgui_refinement"] = serde_json::json!("wrong");
                value
            },
        ] {
            assert!(validate_policy(&changed.to_string()).is_err());
        }
    }
}
