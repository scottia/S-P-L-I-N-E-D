use crate::pipeline::PipelineCandidate;
use crate::source_policy::SourcePolicyConfig;
use image::imageops::FilterType;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

const LOCAL_SOURCES: &[&str] = &["local", "local-library", "embedded", "webpstill"];
const EXACT_RELEASE_SOURCES: &[&str] = &["coverartarchive"];
const FRONT_TYPES: &[&str] = &["front", "front cover"];
const NON_FRONT_TYPES: &[&str] = &[
    "back", "booklet", "medium", "disc", "tray", "obi", "spine", "track",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrictContentDecision {
    pub status: String,
    pub reason: String,
    pub preferred_eligible: bool,
    pub auto_eligible: bool,
    pub match_distance: Option<u32>,
    pub match_source: String,
    pub local_validated: bool,
}

impl Default for StrictContentDecision {
    fn default() -> Self {
        Self {
            status: "not-applicable".to_string(),
            reason: String::new(),
            preferred_eligible: true,
            auto_eligible: true,
            match_distance: None,
            match_source: String::new(),
            local_validated: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct VisualSignature {
    width: u32,
    height: u32,
    dhash: u64,
    ahash: u64,
}

fn image_signature(path: &Path) -> Result<VisualSignature, String> {
    let image = image::open(path)
        .map_err(|error| format!("content analysis failed for {}: {error}", path.display()))?
        .to_luma8();
    let (width, height) = image.dimensions();
    let difference = image::imageops::resize(&image, 9, 8, FilterType::Lanczos3);
    let average = image::imageops::resize(&image, 8, 8, FilterType::Lanczos3);

    let mut dhash = 0_u64;
    for row in 0..8 {
        for column in 0..8 {
            dhash = (dhash << 1)
                | u64::from(
                    difference.get_pixel(column, row)[0] > difference.get_pixel(column + 1, row)[0],
                );
        }
    }

    let average_value = average
        .pixels()
        .map(|pixel| u64::from(pixel[0]))
        .sum::<u64>()
        / 64;
    let mut ahash = 0_u64;
    for pixel in average.pixels() {
        ahash = (ahash << 1) | u64::from(u64::from(pixel[0]) >= average_value);
    }

    Ok(VisualSignature {
        width,
        height,
        dhash,
        ahash,
    })
}

fn full_frame_match(left: VisualSignature, right: VisualSignature) -> (bool, u32, u32) {
    let left_ratio = f64::from(left.width) / f64::from(left.height.max(1));
    let right_ratio = f64::from(right.width) / f64::from(right.height.max(1));
    let ratio_delta = (left_ratio - right_ratio).abs() / left_ratio.max(right_ratio);
    let dhash_distance = (left.dhash ^ right.dhash).count_ones();
    let ahash_distance = (left.ahash ^ right.ahash).count_ones();
    let matched = ratio_delta <= 0.06
        && (dhash_distance <= 10 || (dhash_distance <= 16 && ahash_distance <= 9));
    (matched, dhash_distance, ahash_distance)
}

fn has_type(candidate: &PipelineCandidate, expected: &[&str]) -> bool {
    candidate.reference.types.iter().any(|value| {
        expected
            .iter()
            .any(|item| value.trim().eq_ignore_ascii_case(item))
    })
}

fn is_front(candidate: &PipelineCandidate) -> bool {
    candidate.reference.front || has_type(candidate, FRONT_TYPES)
}

fn is_non_front(candidate: &PipelineCandidate) -> bool {
    !is_front(candidate) && has_type(candidate, NON_FRONT_TYPES)
}

fn is_source(candidate: &PipelineCandidate, sources: &[&str]) -> bool {
    sources.iter().any(|source| {
        candidate
            .downloaded
            .candidate
            .source
            .eq_ignore_ascii_case(source)
    })
}

fn strict_enabled(
    candidate: &PipelineCandidate,
    policies: &BTreeMap<String, SourcePolicyConfig>,
) -> bool {
    policies
        .get(&candidate.downloaded.candidate.source.to_ascii_lowercase())
        .is_some_and(|policy| policy.strict_override)
}

fn best_match(
    signature: VisualSignature,
    references: &[(usize, VisualSignature)],
) -> Option<(usize, u32, u32)> {
    references
        .iter()
        .filter_map(|(index, reference)| {
            let (matched, dhash, ahash) = full_frame_match(signature, *reference);
            matched.then_some((*index, dhash, ahash))
        })
        .min_by_key(|(_, dhash, ahash)| (*dhash, *ahash))
}

pub fn apply(
    candidates: &mut [PipelineCandidate],
    policies: &BTreeMap<String, SourcePolicyConfig>,
) {
    for candidate in candidates.iter_mut() {
        candidate.strict = StrictContentDecision::default();
    }

    let strict_indices = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| strict_enabled(candidate, policies).then_some(index))
        .collect::<Vec<_>>();
    if strict_indices.is_empty() {
        return;
    }

    let mut signatures = Vec::with_capacity(candidates.len());
    for (index, candidate) in candidates.iter_mut().enumerate() {
        match image_signature(candidate.downloaded.path()) {
            Ok(signature) => signatures.push(Some(signature)),
            Err(error) => {
                signatures.push(None);
                if strict_indices.contains(&index) {
                    candidate.strict.status = "analysis-error".to_string();
                    candidate.strict.reason = error;
                    candidate.strict.preferred_eligible = false;
                    candidate.strict.auto_eligible = false;
                }
            }
        }
    }

    let exact_front = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            (is_source(candidate, EXACT_RELEASE_SOURCES) && is_front(candidate))
                .then(|| signatures[index].map(|signature| (index, signature)))
                .flatten()
        })
        .collect::<Vec<_>>();
    let exact_non_front = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            (is_source(candidate, EXACT_RELEASE_SOURCES) && is_non_front(candidate))
                .then(|| signatures[index].map(|signature| (index, signature)))
                .flatten()
        })
        .collect::<Vec<_>>();
    let flexible_front = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            (!is_source(candidate, LOCAL_SOURCES)
                && !strict_enabled(candidate, policies)
                && is_front(candidate))
            .then(|| signatures[index].map(|signature| (index, signature)))
            .flatten()
        })
        .collect::<Vec<_>>();

    let local_indices = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| {
            (is_source(candidate, LOCAL_SOURCES) && signatures[index].is_some()).then_some(index)
        })
        .collect::<Vec<_>>();
    let mut validated_local = Vec::new();
    for index in local_indices {
        let signature = signatures[index].expect("local signature was checked");
        let exact_match = best_match(signature, &exact_front);
        let agreeing_sources = flexible_front
            .iter()
            .filter(|(_, reference)| full_frame_match(signature, *reference).0)
            .map(|(reference_index, _)| {
                candidates[*reference_index]
                    .downloaded
                    .candidate
                    .source
                    .to_ascii_lowercase()
            })
            .collect::<HashSet<_>>();
        if exact_match.is_some() || agreeing_sources.len() >= 2 {
            let decision = &mut candidates[index].strict;
            decision.status = "validated-local".to_string();
            decision.reason = if exact_match.is_some() {
                "matches exact-release CAA Front".to_string()
            } else {
                "matches multiple independent front-art sources".to_string()
            };
            decision.local_validated = true;
            validated_local.push((index, signature));
        }
    }

    for index in strict_indices {
        let Some(signature) = signatures[index] else {
            continue;
        };
        let front_match = best_match(signature, &exact_front);
        let local_match = best_match(signature, &validated_local);
        let wrong_type = best_match(signature, &exact_non_front);
        let agreeing_sources = flexible_front
            .iter()
            .filter(|(reference_index, reference)| {
                *reference_index != index && full_frame_match(signature, *reference).0
            })
            .map(|(reference_index, _)| {
                candidates[*reference_index]
                    .downloaded
                    .candidate
                    .source
                    .to_ascii_lowercase()
            })
            .collect::<HashSet<_>>();

        let approved_front = candidates[index].reference.approved && is_front(&candidates[index]);
        let decision = &mut candidates[index].strict;
        if let Some((reference_index, distance, _)) = front_match {
            decision.status = "validated-front".to_string();
            decision.reason = "matches exact-release CAA Front artwork".to_string();
            decision.preferred_eligible = true;
            decision.auto_eligible = approved_front;
            decision.match_distance = Some(distance);
            decision.match_source = candidates[reference_index]
                .downloaded
                .candidate
                .source
                .clone();
        } else if let Some((reference_index, distance, _)) = local_match {
            decision.status = "validated-local".to_string();
            decision.reason = "matches locally cached, source-validated front artwork".to_string();
            decision.preferred_eligible = true;
            decision.auto_eligible = false;
            decision.match_distance = Some(distance);
            decision.match_source = candidates[reference_index]
                .downloaded
                .candidate
                .source
                .clone();
        } else if agreeing_sources.len() >= 2 {
            decision.status = "validated-consensus".to_string();
            decision.reason = "matches multiple independent front-art sources".to_string();
            decision.preferred_eligible = true;
            decision.auto_eligible = false;
        } else if let Some((reference_index, distance, _)) = wrong_type {
            decision.status = "wrong-type".to_string();
            decision.reason = "matches exact-release non-Front artwork".to_string();
            decision.preferred_eligible = false;
            decision.auto_eligible = false;
            decision.match_distance = Some(distance);
            decision.match_source = candidates[reference_index]
                .downloaded
                .candidate
                .source
                .clone();
        } else {
            decision.status = "unverified".to_string();
            decision.reason =
                "no full-frame exact-release Front or trusted consensus match".to_string();
            decision.preferred_eligible = false;
            decision.auto_eligible = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::download::DownloadedCandidate;
    use crate::source::ArtworkReference;
    use image::{ImageBuffer, Rgb, RgbImage};
    use tempfile::TempDir;

    fn cover(path: &Path, width: u32, height: u32) {
        let mut image: RgbImage = ImageBuffer::from_pixel(width, height, Rgb([232, 210, 173]));
        for y in height / 5..height * 4 / 5 {
            for x in width / 5..width * 4 / 5 {
                image.put_pixel(x, y, Rgb([22, 44, 85]));
            }
        }
        image.save(path).unwrap();
    }

    fn fixture(path: &Path, source: &str, front: bool, types: &[&str]) -> PipelineCandidate {
        PipelineCandidate {
            reference: ArtworkReference {
                source: source.to_string(),
                id: source.to_string(),
                url: String::new(),
                front,
                approved: true,
                types: types.iter().map(|value| value.to_string()).collect(),
            },
            downloaded: DownloadedCandidate::from_existing_path(source, path.to_path_buf(), 0, "")
                .unwrap(),
            strict: StrictContentDecision::default(),
        }
    }

    fn policies() -> BTreeMap<String, SourcePolicyConfig> {
        BTreeMap::from([(
            "amazon".to_string(),
            SourcePolicyConfig {
                strict_override: true,
                ..SourcePolicyConfig::default()
            },
        )])
    }

    #[test]
    fn exact_front_match_is_preferred_and_auto_eligible() {
        let root = TempDir::new().unwrap();
        let caa = root.path().join("caa.png");
        let amazon = root.path().join("amazon.png");
        cover(&caa, 600, 600);
        image::open(&caa)
            .unwrap()
            .resize_exact(1200, 1200, FilterType::Lanczos3)
            .save(&amazon)
            .unwrap();
        let mut candidates = vec![
            fixture(&caa, "coverartarchive", true, &["Front"]),
            fixture(&amazon, "amazon", true, &["Front"]),
        ];
        apply(&mut candidates, &policies());
        assert_eq!(candidates[1].strict.status, "validated-front");
        assert!(candidates[1].strict.preferred_eligible);
        assert!(candidates[1].strict.auto_eligible);
    }

    #[test]
    fn product_photo_remains_visible_but_is_not_preferred() {
        let root = TempDir::new().unwrap();
        let caa = root.path().join("caa.png");
        let product = root.path().join("product.png");
        cover(&caa, 600, 600);
        let mut table: RgbImage = ImageBuffer::from_pixel(800, 600, Rgb([155, 148, 138]));
        let small = image::open(&caa)
            .unwrap()
            .resize_exact(360, 360, FilterType::Lanczos3)
            .to_rgb8();
        image::imageops::overlay(&mut table, &small, 220, 120);
        table.save(&product).unwrap();
        let mut candidates = vec![
            fixture(&caa, "coverartarchive", true, &["Front"]),
            fixture(&product, "amazon", true, &["Front"]),
        ];
        apply(&mut candidates, &policies());
        assert_eq!(candidates[1].strict.status, "unverified");
        assert!(!candidates[1].strict.preferred_eligible);
        assert!(!candidates[1].strict.auto_eligible);
    }
}
