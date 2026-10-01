use crate::source::{
    ArtworkProvider, ArtworkQuery, ArtworkReference, ProviderContext, ProviderDiscoveryFuture,
};
use reqwest::{Client, StatusCode};
use std::collections::HashSet;

const SEARCH_URL: &str = "https://www.amazon.com/s";
const IMAGE_PREFIX: &str = "https://m.media-amazon.com/images/I/";

pub struct AmazonStore {
    client: Client,
}

impl AmazonStore {
    pub fn new() -> Result<Self, String> {
        let client = Client::builder()
            .user_agent(concat!("SPLINED/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| format!("Unable to create Amazon Store HTTP client: {error}"))?;
        Ok(Self { client })
    }

    async fn search(&self, context: &ProviderContext) -> Result<Vec<ArtworkReference>, String> {
        let title = context
            .release_group_title
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(context.release_title.as_str());
        let query = format!("{} {}", context.artist_credit.trim(), title.trim())
            .trim()
            .to_string();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let response = self
            .client
            .get(SEARCH_URL)
            .query(&[("k", query.as_str()), ("i", "popular")])
            .header("Accept", "text/html,application/xhtml+xml")
            .header("Accept-Language", "en-US,en;q=0.8")
            .send()
            .await
            .map_err(|error| format!("Unable to query Amazon Store: {error}"))?;
        if response.status() != StatusCode::OK {
            return Err(format!(
                "Amazon Store search returned HTTP {}",
                response.status()
            ));
        }
        let body = response
            .text()
            .await
            .map_err(|error| format!("Unable to read Amazon Store results: {error}"))?;
        if body.contains("validateCaptcha") || body.contains("api-services-support@amazon.com") {
            return Err("Amazon Store blocked the public search request".to_string());
        }
        let parsed = parse_search_results(&body, title);
        if parsed.result_blocks > 0 && parsed.image_tags == 0 {
            return Err(format!(
                "Amazon Store result markup contained no primary image tags (blocks={})",
                parsed.result_blocks
            ));
        }
        if parsed.image_tags > 0 && parsed.normalized_images == 0 {
            return Err(format!(
                "Amazon Store result markup contained no supported primary image URLs (blocks={} image_tags={})",
                parsed.result_blocks, parsed.image_tags
            ));
        }
        Ok(parsed.references)
    }
}

impl ArtworkProvider for AmazonStore {
    fn name(&self) -> &'static str {
        "amazon"
    }

    fn discover<'a>(
        &'a self,
        _query: &'a ArtworkQuery,
        context: &'a ProviderContext,
    ) -> ProviderDiscoveryFuture<'a> {
        Box::pin(async move { self.search(context).await })
    }
}

#[derive(Default)]
struct ParsedSearchResults {
    references: Vec<ArtworkReference>,
    result_blocks: usize,
    image_tags: usize,
    normalized_images: usize,
}

fn parse_search_results(body: &str, expected_title: &str) -> ParsedSearchResults {
    let mut starts = [
        "data-component-type=\"s-search-result\"",
        "data-component-type='s-search-result'",
    ]
    .iter()
    .flat_map(|marker| body.match_indices(marker).map(|(index, _)| index))
    .collect::<Vec<_>>();
    starts.sort_unstable();
    starts.dedup();
    let result_blocks = starts.len();
    starts.push(body.len());
    let mut references = Vec::new();
    let mut seen = HashSet::new();
    let mut image_tags = 0;
    let mut normalized_images = 0;

    for pair in starts.windows(2) {
        let block = &body[pair[0]..pair[1]];
        let Some(image) = primary_image_tag(block) else {
            continue;
        };
        image_tags += 1;
        let Some(url) = primary_image_url(image) else {
            continue;
        };
        normalized_images += 1;
        let alt = attribute_value(image, "alt").unwrap_or_default();
        if !alt.is_empty() && !title_matches(&alt, expected_title) {
            continue;
        }
        if !seen.insert(url.clone()) {
            continue;
        }
        let id = url
            .strip_prefix(IMAGE_PREFIX)
            .and_then(|value| value.rsplit_once('.').map(|(asset, _)| asset))
            .unwrap_or_default()
            .to_string();
        references.push(ArtworkReference {
            source: "amazon".to_string(),
            id,
            url,
            front: true,
            approved: true,
            types: vec!["Front".to_string()],
        });
        if references.len() >= 6 {
            break;
        }
    }
    ParsedSearchResults {
        references,
        result_blocks,
        image_tags,
        normalized_images,
    }
}

fn primary_image_tag(block: &str) -> Option<&str> {
    let mut offset = 0;
    while let Some(relative_start) = block[offset..].find("<img") {
        let start = offset + relative_start;
        let end = block[start..]
            .find('>')
            .map(|relative_end| start + relative_end + 1)?;
        let tag = &block[start..end];
        let is_primary = attribute_value(tag, "class")
            .is_some_and(|classes| classes.split_whitespace().any(|class| class == "s-image"));
        if is_primary {
            return Some(tag);
        }
        offset = end;
    }
    None
}

fn primary_image_url(tag: &str) -> Option<String> {
    ["src", "data-src"]
        .into_iter()
        .filter_map(|attribute| attribute_value(tag, attribute))
        .find_map(|value| original_image_url(&value))
        .or_else(|| {
            attribute_value(tag, "srcset").and_then(|srcset| {
                srcset
                    .split(',')
                    .find_map(|entry| entry.split_whitespace().next().and_then(original_image_url))
            })
        })
}

fn attribute_value(tag: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let needle = format!("{name}={quote}");
        if let Some(position) = tag.find(&needle) {
            let start = position + needle.len();
            if let Some(length) = tag[start..].find(quote) {
                return Some(html_decode(&tag[start..start + length]));
            }
        }
    }
    None
}

fn html_decode(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn original_image_url(value: &str) -> Option<String> {
    let raw = html_decode(value.trim());
    let path = raw.split(['?', '#']).next()?;
    let filename = path.strip_prefix(IMAGE_PREFIX)?;
    if filename.contains('/') || filename.is_empty() {
        return None;
    }
    let (asset, extension) = if let Some(transform) = filename.find("._") {
        let extension = filename.rsplit_once('.')?.1;
        (&filename[..transform], extension)
    } else {
        filename.rsplit_once('.')?
    };
    if asset.is_empty()
        || !matches!(
            extension.to_ascii_lowercase().as_str(),
            "jpg" | "jpeg" | "png"
        )
    {
        return None;
    }
    Some(format!(
        "{IMAGE_PREFIX}{asset}.{}",
        extension.to_ascii_lowercase()
    ))
}

fn normalized(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn title_matches(found: &str, requested: &str) -> bool {
    let found = normalized(found);
    let requested = normalized(requested);
    !found.is_empty()
        && !requested.is_empty()
        && (found == requested || found.starts_with(&requested) || requested.starts_with(&found))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_amazon_image_transform() {
        assert_eq!(
            original_image_url("https://m.media-amazon.com/images/I/81Y+xtQACkL._AC_UY218_.jpg"),
            Some("https://m.media-amazon.com/images/I/81Y+xtQACkL.jpg".to_string())
        );
        assert_eq!(
            original_image_url("https://m.media-amazon.com/images/I/81Y+xtQACkL.jpg"),
            Some("https://m.media-amazon.com/images/I/81Y+xtQACkL.jpg".to_string())
        );
        assert!(original_image_url("https://example.invalid/images/I/no.jpg").is_none());
    }

    #[test]
    fn parses_primary_result_image() {
        let html = r#"<div data-component-type="s-search-result">
          <img class="s-image" alt="A Summer Place" src="https://m.media-amazon.com/images/I/81Y+xtQACkL._AC_UY218_.jpg">
        </div>"#;
        let results = parse_search_results(html, "A Summer Place");
        assert_eq!(results.references.len(), 1);
        assert_eq!(results.references[0].source, "amazon");
        assert_eq!(results.result_blocks, 1);
        assert_eq!(results.image_tags, 1);
        assert_eq!(results.normalized_images, 1);
    }

    #[test]
    fn skips_amazon_image_wrapper_and_finds_actual_primary_image() {
        let html = r#"<div data-component-type="s-search-result">
          <div class="a-section s-image-fixed-height">
            <span class="rush-component s-latency-cf-section">
              <img class="s-image" alt="Heart Of Stone"
                   src="https://m.media-amazon.com/images/I/61h9ycBlFuL._AC_UY218_.jpg">
            </span>
          </div>
        </div>"#;
        let results = parse_search_results(html, "Heart of Stone");
        assert_eq!(results.references.len(), 1);
        assert_eq!(results.image_tags, 1);
        assert_eq!(results.normalized_images, 1);
        assert_eq!(
            results.references[0].url,
            "https://m.media-amazon.com/images/I/61h9ycBlFuL.jpg"
        );
    }

    #[test]
    fn reads_lazy_primary_image_attributes() {
        let html = r#"<div data-component-type='s-search-result'>
          <img class='a-lazy-loaded s-image' alt='Heart Of Stone'
               src='data:image/gif;base64,placeholder'
               data-src='https://m.media-amazon.com/images/I/61h9ycBlFuL._AC_UY218_.jpg'>
        </div>"#;
        let results = parse_search_results(html, "Heart of Stone");
        assert_eq!(results.references.len(), 1);
        assert_eq!(
            results.references[0].url,
            "https://m.media-amazon.com/images/I/61h9ycBlFuL.jpg"
        );
    }
}
