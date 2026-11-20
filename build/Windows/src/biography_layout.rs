//! Native Windows paragraph composition for Artist biographies.
//!
//! Slint 1.18 does not expose justified paragraph layout.  DirectWrite is the
//! monitor-aware Windows text authority, so it performs shaping, wrapping and
//! justification with the same Segoe UI face that the native workspace uses.
//! English soft-hyphen opportunities are supplied from the embedded
//! Knuth-Liang dictionary.  The returned positions are rendered by Slint; the
//! source biography is never modified or saved with discretionary hyphens.

use hyphenation::{Hyphenator, Language, Load, Standard};
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq)]
pub struct PositionedWord {
    pub text: String,
    pub x: f32,
    pub width: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParagraphLine {
    pub words: Vec<PositionedWord>,
    pub justified: bool,
    pub extent: f32,
}

static ENGLISH_US: OnceLock<Option<Standard>> = OnceLock::new();

fn english_us() -> Option<&'static Standard> {
    ENGLISH_US
        .get_or_init(|| Standard::from_embedded(Language::EnglishUS).ok())
        .as_ref()
}

fn discretionary_hyphens(token: &str) -> String {
    let Some(dictionary) = english_us() else {
        return token.to_string();
    };
    let Some(first) = token.find(char::is_alphabetic) else {
        return token.to_string();
    };
    let last = token
        .char_indices()
        .rev()
        .find(|(_, character)| character.is_alphabetic())
        .map(|(offset, character)| offset + character.len_utf8())
        .unwrap_or(first);
    let core = &token[first..last];
    if core.chars().count() < 6 {
        return token.to_string();
    }

    let mut breaks = dictionary.hyphenate(core).breaks;
    // Never leave one-character fragments at either end.  The dictionary is
    // conservative already; this extra guard keeps names and abbreviations
    // visually natural.
    breaks.retain(|offset| {
        core[..*offset].chars().count() >= 2 && core[*offset..].chars().count() >= 2
    });
    if breaks.is_empty() {
        return token.to_string();
    }

    let mut result = core.to_string();
    for offset in breaks.into_iter().rev() {
        result.insert(offset, '\u{00ad}');
    }
    format!("{}{}{}", &token[..first], result, &token[last..])
}

fn prepare_text(text: &str) -> String {
    let mut prepared = String::with_capacity(text.len() + text.len() / 12);
    let mut token = String::new();
    for character in text.chars() {
        if character.is_whitespace() {
            if !token.is_empty() {
                prepared.push_str(&discretionary_hyphens(&token));
                token.clear();
            }
            prepared.push(character);
        } else {
            token.push(character);
        }
    }
    if !token.is_empty() {
        prepared.push_str(&discretionary_hyphens(&token));
    }
    prepared
}

#[cfg(windows)]
pub fn compose(text: &str, width: f32, font_size: f32) -> Option<Vec<ParagraphLine>> {
    use windows::Win32::Graphics::DirectWrite::{
        DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
        DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_HIT_TEST_METRICS, DWRITE_LINE_METRICS,
        DWRITE_TEXT_ALIGNMENT_JUSTIFIED, DWRITE_WORD_WRAPPING_WRAP, DWriteCreateFactory,
        IDWriteFactory,
    };
    use windows::core::w;

    let prepared = prepare_text(text);
    let utf16 = prepared.encode_utf16().collect::<Vec<_>>();
    if utf16.is_empty() {
        return Some(Vec::new());
    }

    // SAFETY: DirectWrite objects own their COM lifetimes and all buffer
    // lengths passed below are derived directly from the backing UTF-16 slice.
    unsafe {
        let factory = DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED).ok()?;
        let format = factory
            .CreateTextFormat(
                w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                font_size.clamp(8.0, 48.0),
                w!("en-US"),
            )
            .ok()?;
        format
            .SetTextAlignment(DWRITE_TEXT_ALIGNMENT_JUSTIFIED)
            .ok()?;
        format.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP).ok()?;
        let layout = factory
            .CreateTextLayout(&utf16, &format, width.max(80.0), 16_384.0)
            .ok()?;

        let mut line_count = 0;
        // The sizing call intentionally returns E_NOT_SUFFICIENT_BUFFER while
        // still reporting the required count, following the DirectWrite API.
        let _ = layout.GetLineMetrics(None, &mut line_count);
        if line_count == 0 {
            return None;
        }
        let mut metrics = vec![DWRITE_LINE_METRICS::default(); line_count as usize];
        layout
            .GetLineMetrics(Some(metrics.as_mut_slice()), &mut line_count)
            .ok()?;
        metrics.truncate(line_count as usize);

        let mut lines = Vec::with_capacity(metrics.len());
        let mut line_start = 0usize;
        for metric in metrics {
            let line_end = line_start
                .saturating_add(metric.length as usize)
                .min(utf16.len());
            let visible_end = line_end
                .saturating_sub(metric.newlineLength as usize)
                .saturating_sub(metric.trailingWhitespaceLength as usize);
            let paragraph_final = metric.newlineLength > 0 || line_end == utf16.len();
            let mut words = Vec::new();
            let mut cursor = line_start;

            while cursor < visible_end {
                while cursor < visible_end
                    && char::from_u32(utf16[cursor] as u32).is_some_and(char::is_whitespace)
                {
                    cursor += 1;
                }
                if cursor >= visible_end {
                    break;
                }
                let start = cursor;
                while cursor < visible_end
                    && !char::from_u32(utf16[cursor] as u32).is_some_and(char::is_whitespace)
                {
                    cursor += 1;
                }
                let end = cursor;
                let broken_at_soft_hyphen =
                    end == visible_end && end > start && utf16[end.saturating_sub(1)] == 0x00ad;
                let mut word = String::from_utf16_lossy(&utf16[start..end]).replace('\u{00ad}', "");
                if broken_at_soft_hyphen {
                    word.push('-');
                }
                if word.is_empty() {
                    continue;
                }

                let mut start_x = 0.0;
                let mut start_y = 0.0;
                let mut start_metrics = DWRITE_HIT_TEST_METRICS::default();
                layout
                    .HitTestTextPosition(
                        start as u32,
                        false,
                        &mut start_x,
                        &mut start_y,
                        &mut start_metrics,
                    )
                    .ok()?;
                let mut end_x = start_x + start_metrics.width;
                if end > start {
                    let mut trailing_x = 0.0;
                    let mut trailing_y = 0.0;
                    let mut trailing_metrics = DWRITE_HIT_TEST_METRICS::default();
                    layout
                        .HitTestTextPosition(
                            end.saturating_sub(1) as u32,
                            true,
                            &mut trailing_x,
                            &mut trailing_y,
                            &mut trailing_metrics,
                        )
                        .ok()?;
                    end_x = trailing_x;
                }
                words.push(PositionedWord {
                    text: word,
                    x: start_x.max(0.0),
                    width: (end_x - start_x).max(start_metrics.width).max(1.0),
                });
            }

            let extent = words
                .last()
                .map(|word| word.x + word.width)
                .unwrap_or_default();
            lines.push(ParagraphLine {
                justified: !paragraph_final && words.len() > 1,
                words,
                extent,
            });
            line_start = line_end;
        }
        Some(lines)
    }
}

#[cfg(not(windows))]
pub fn compose(text: &str, width: f32, font_size: f32) -> Option<Vec<ParagraphLine>> {
    let average = (font_size * 0.52).max(1.0);
    let columns = (width / average).floor().max(8.0) as usize;
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let words = paragraph.split_whitespace().collect::<Vec<_>>();
        let mut offset = 0;
        while offset < words.len() {
            let mut count = 0usize;
            let mut taken = 0usize;
            while offset + taken < words.len() {
                let proposed =
                    count + words[offset + taken].chars().count() + usize::from(taken > 0);
                if taken > 0 && proposed > columns {
                    break;
                }
                count = proposed;
                taken += 1;
            }
            let last = offset + taken == words.len();
            let mut x = 0.0;
            let positioned = words[offset..offset + taken]
                .iter()
                .map(|word| {
                    let width = word.chars().count() as f32 * average;
                    let positioned = PositionedWord {
                        text: (*word).to_string(),
                        x,
                        width,
                    };
                    x += width + average;
                    positioned
                })
                .collect();
            let extent = positioned
                .last()
                .map(|word| word.x + word.width)
                .unwrap_or_default();
            lines.push(ParagraphLine {
                words: positioned,
                justified: !last,
                extent,
            });
            offset += taken;
        }
    }
    Some(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discretionary_hyphens_do_not_change_visible_copy() {
        let prepared = prepare_text("international collaboration, punctuation intact.");
        assert!(prepared.contains('\u{00ad}'));
        assert_eq!(
            prepared.replace('\u{00ad}', ""),
            "international collaboration, punctuation intact."
        );
    }

    #[test]
    fn narrower_paragraph_produces_more_lines() {
        let copy =
            "Natural typography should remain readable while the artist overview changes width.";
        let narrow = compose(copy, 260.0, 15.0).unwrap();
        let wide = compose(copy, 620.0, 15.0).unwrap();
        assert!(narrow.len() > wide.len());
        assert!(!wide.last().unwrap().justified);
    }

    #[test]
    fn directwrite_justification_avoids_exaggerated_word_gaps() {
        let copy = "By the mid-'90s, the grunge revolution had reached every nook and cranny of the United States, including the tiny town of Escatawpa, Mississippi. Its members forged a sound inspired by their Seattle idols.";
        let lines = compose(copy, 560.0, 15.0).unwrap();
        assert!(lines.len() >= 2);
        for line in lines.iter().filter(|line| line.justified) {
            for pair in line.words.windows(2) {
                let gap = pair[1].x - (pair[0].x + pair[0].width);
                assert!(gap >= 3.0, "collapsed justified gap: {gap}");
                assert!(gap <= 22.5, "exaggerated justified gap: {gap}");
            }
        }
    }
}
