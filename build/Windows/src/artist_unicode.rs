use std::collections::HashMap;
use std::sync::OnceLock;
use unicode_normalization::UnicodeNormalization;

const _: () = assert!(
    unicode_normalization::UNICODE_VERSION.0 == 18
        && unicode_normalization::UNICODE_VERSION.1 == 0
        && unicode_normalization::UNICODE_VERSION.2 == 0
);
const DATA: &str = include_str!("../../WEBGui/webgui/artist_unicode_data.txt");

#[derive(Default)]
struct UnicodeData {
    folds: HashMap<char, String>,
    alphanumeric: Vec<(u32, u32)>,
    format: Vec<(u32, u32)>,
}

fn data() -> &'static UnicodeData {
    static DATASET: OnceLock<UnicodeData> = OnceLock::new();
    DATASET.get_or_init(|| {
        let mut data = UnicodeData::default();
        let codepoint = |value: &str| u32::from_str_radix(value, 16).expect("Unicode hex");
        for row in DATA.lines().filter(|line| !line.starts_with('#')) {
            let mut fields = row.split_ascii_whitespace();
            let kind = fields.next().expect("Unicode table row type");
            let start = codepoint(fields.next().expect("Unicode range/codepoint start"));
            match kind {
                "F" => {
                    let folded = fields
                        .map(|item| char::from_u32(codepoint(item)).expect("Unicode fold scalar"))
                        .collect();
                    data.folds
                        .insert(char::from_u32(start).expect("Unicode scalar"), folded);
                }
                "A" | "G" => {
                    let end = codepoint(fields.next().expect("Unicode range end"));
                    let ranges = if kind == "A" {
                        &mut data.alphanumeric
                    } else {
                        &mut data.format
                    };
                    ranges.push((start, end));
                }
                _ => panic!("Unexpected Unicode table row type"),
            }
        }
        data
    })
}

fn contains(ranges: &[(u32, u32)], value: char) -> bool {
    let value = u32::from(value);
    ranges
        .get(ranges.partition_point(|&(_, end)| end < value))
        .is_some_and(|&(start, _)| start <= value)
}

pub(crate) fn casefold(value: &str) -> String {
    if value.is_ascii() {
        return value.to_ascii_lowercase();
    }
    let folds = &data().folds;
    let mut result = String::with_capacity(value.len());
    for character in value.chars() {
        if let Some(folded) = folds.get(&character) {
            result.push_str(folded);
        } else {
            result.push(character);
        }
    }
    result
}

pub(crate) fn alphanumeric(character: char) -> bool {
    if character.is_ascii() {
        character.is_ascii_alphanumeric()
    } else {
        contains(&data().alphanumeric, character)
    }
}

pub(crate) fn normalized_fold(value: &str) -> String {
    if value.is_ascii() {
        return value.to_ascii_lowercase();
    }
    casefold(&value.nfkc().collect::<String>())
}

pub(crate) fn search_literal(value: &str) -> String {
    normalized_fold(value)
        .trim_matches(|character: char| {
            character.is_whitespace() || matches!(character, '\x1c'..='\x1f')
        })
        .to_string()
}

pub(crate) fn sort_key(value: &str) -> String {
    let visible: String = value
        .nfkc()
        .filter(|&character| !contains(&data().format, character))
        .collect();
    casefold(&visible)
        .trim_matches(|character: char| {
            character.is_whitespace() || matches!(character, '\x1c'..='\x1f')
        })
        .to_string()
}
