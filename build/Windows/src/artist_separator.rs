//! ART-004 separator grammar. The COMP tag and filename matchers use their
//! established native expressions independently of this catalog grammar.

use regex::Regex;

// Unicode White_Space, explicitly spelled out for both regex engines.
const SPACE: &str = "\t\n\u{000b}\u{000c}\r \u{0085}\u{00a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}";
const MAX_PROFILE_BYTES: usize = 4096;
const MAX_GROUP_DEPTH: usize = 32;

pub const REMEDIATION: &str = "Incompatible Artist separator profile. Configured credit splitting and catalog refresh are paused. In Settings → Library → Advanced, edit or import a portable profile, save it, then follow the catalog reset and refresh instructions.";

#[derive(Default)]
pub struct ArtistSeparator {
    regex: Option<Regex>,
    whole: Option<Regex>,
    ignore_ascii_case: bool,
}

impl ArtistSeparator {
    /// Both runtimes compile the same restricted, nonempty delimiter grammar:
    /// literal text, positive ASCII classes, \s, groups, alternatives and
    /// ?/*/+ repetition. Only a leading (?i) is allowed, with ASCII folding.
    pub fn parse(profile: &str) -> Result<Self, &'static str> {
        if profile.len() > MAX_PROFILE_BYTES {
            return Err(REMEDIATION);
        }
        let (pattern, ignore_ascii_case) = profile
            .strip_prefix("(?i)")
            .map_or((profile, false), |rest| (rest, true));
        let mut chars = pattern.chars().peekable();
        let mut portable = String::new();
        let mut depth = 0;
        let mut can_repeat = false;
        let mut can_repeat_many = false;
        let mut has_atom = false;
        while let Some(ch) = chars.next() {
            match ch {
                '(' => {
                    if depth == MAX_GROUP_DEPTH
                        || chars.next_if_eq(&'?').is_some() && chars.next() != Some(':')
                    {
                        return Err(REMEDIATION);
                    }
                    portable.push_str("(?:");
                    depth += 1;
                    has_atom = false;
                    can_repeat = false;
                }
                ')' | '|' => {
                    if !has_atom || ch == ')' && depth == 0 {
                        return Err(REMEDIATION);
                    }
                    portable.push(ch);
                    if ch == ')' {
                        depth -= 1;
                    }
                    has_atom = ch == ')';
                    can_repeat = has_atom;
                    can_repeat_many = false;
                }
                '?' | '*' | '+' => {
                    if !can_repeat || ch != '?' && !can_repeat_many {
                        return Err(REMEDIATION);
                    }
                    portable.push(ch);
                    can_repeat = false;
                }
                '.' | '^' | '$' | '{' | '}' | ']' => return Err(REMEDIATION),
                _ => {
                    can_repeat_many = true;
                    match ch {
                        '[' => portable.push_str(&character_class(&mut chars, ignore_ascii_case)?),
                        '\\' => match chars.next() {
                            Some('s') => portable.push_str(&format!("[{SPACE}]")),
                            ch => portable.push_str(&literal(escaped(ch)?, ignore_ascii_case)?),
                        },
                        ch => portable.push_str(&literal(ch, ignore_ascii_case)?),
                    }
                    has_atom = true;
                    can_repeat = true;
                }
            }
        }
        if depth != 0 || !has_atom {
            return Err(REMEDIATION);
        }
        let regex = Regex::new(&portable).map_err(|_| REMEDIATION)?;
        if regex.is_match("") {
            return Err(REMEDIATION);
        }
        let whole = Regex::new(&format!(r"\A(?:{portable})\z")).map_err(|_| REMEDIATION)?;
        Ok(Self {
            regex: Some(regex),
            whole: Some(whole),
            ignore_ascii_case,
        })
    }

    pub fn warning(profile: &str) -> Option<&'static str> {
        Self::parse(profile).err()
    }

    pub(crate) fn matches(&self, value: &str) -> Vec<(usize, usize)> {
        let Some(regex) = &self.regex else {
            return Vec::new();
        };
        // ASCII folding preserves byte offsets in the original text.
        let folded;
        let input = if self.ignore_ascii_case {
            folded = value.to_ascii_lowercase();
            &folded
        } else {
            value
        };
        regex
            .find_iter(input)
            .map(|m| (m.start(), m.end()))
            .collect()
    }

    pub(crate) fn split<'a>(&self, value: &'a str) -> Vec<&'a str> {
        let mut last = 0;
        let mut parts = Vec::new();
        for (start, end) in self.matches(value) {
            parts.push(&value[last..start]);
            last = end;
        }
        parts.push(&value[last..]);
        parts
    }

    pub(crate) fn full_match(&self, value: &str) -> bool {
        let folded;
        let input = if self.ignore_ascii_case {
            folded = value.to_ascii_lowercase();
            &folded
        } else {
            value
        };
        self.whole
            .as_ref()
            .is_some_and(|regex| regex.is_match(input))
    }
}

fn escaped(ch: Option<char>) -> Result<char, &'static str> {
    match ch {
        Some('t') => Ok('\t'),
        Some('n') => Ok('\n'),
        Some('r') => Ok('\r'),
        Some(ch) if ch.is_ascii_punctuation() || ch == ' ' => Ok(ch),
        _ => Err(REMEDIATION),
    }
}

fn literal(ch: char, ignore_ascii_case: bool) -> Result<String, &'static str> {
    if ignore_ascii_case && !ch.is_ascii() {
        return Err(REMEDIATION);
    }
    Ok(regex::escape(
        &if ignore_ascii_case {
            ch.to_ascii_lowercase()
        } else {
            ch
        }
        .to_string(),
    ))
}

fn character_class(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    fold: bool,
) -> Result<String, &'static str> {
    let mut members = String::new();
    let member = |ch, chars: &mut std::iter::Peekable<std::str::Chars<'_>>| match ch {
        '\\' => escaped(chars.next()),
        '[' | '^' => Err(REMEDIATION),
        ch if ch.is_ascii() => Ok(ch),
        _ => Err(REMEDIATION),
    };
    let encode = |ch: char| {
        format!(
            r"\x{:02x}",
            u32::from(if fold { ch.to_ascii_lowercase() } else { ch })
        )
    };
    while let Some(ch) = chars.next() {
        if ch == ']' {
            return if members.is_empty() {
                Err(REMEDIATION)
            } else {
                Ok(format!("[{members}]"))
            };
        }
        if matches!(ch, '&' | '|' | '~' | '-') && chars.peek() == Some(&ch) {
            return Err(REMEDIATION);
        }
        if ch == '\\' && chars.next_if_eq(&'s').is_some() {
            members.push_str(SPACE);
            continue;
        }
        let first = member(ch, chars)?;
        if chars.next_if_eq(&'-').is_some() {
            if chars.peek() == Some(&'-') {
                return Err(REMEDIATION);
            }
            if chars.peek() == Some(&']') {
                members.push_str(&encode(first));
                members.push_str(&encode('-'));
                continue;
            }
            let last = member(chars.next().ok_or(REMEDIATION)?, chars)?;
            if first > last {
                return Err(REMEDIATION);
            }
            for ch in first..=last {
                members.push_str(&encode(ch));
            }
        } else {
            members.push_str(&encode(first));
        }
    }
    Err(REMEDIATION)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn art004_portable_separator_matches_shared_cases() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../.github/test-data/artist-identity-conformance.json"
        ))
        .unwrap();
        for case in fixture["portable_separator_cases"].as_array().unwrap() {
            let profile = case["profile"].as_str().unwrap();
            let parsed = ArtistSeparator::parse(profile);
            if let Some(expected) = case["parts"].as_array() {
                let parsed = parsed.unwrap_or_else(|_| panic!("Rejected {profile:?}"));
                let expected: Vec<_> = expected.iter().map(|part| part.as_str().unwrap()).collect();
                assert_eq!(
                    parsed.split(case["value"].as_str().unwrap()),
                    expected,
                    "{profile:?}"
                );
                assert!(
                    parsed.full_match(case["whole"].as_str().unwrap()),
                    "{profile:?}"
                );
            } else {
                assert_eq!(parsed.err(), Some(REMEDIATION), "{profile:?}");
            }
        }
    }
}
