// src/fonts.rs
//! System font discovery and fuzzy matching for terminal font selection.

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFamilyInfo {
    pub name: String,
    pub is_monospaced: bool,
}

/// Discovers all font families installed on the system using fontdb.
pub fn query_system_fonts() -> Vec<FontFamilyInfo> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    // Map family_name -> has_monospaced_face
    let mut families: BTreeMap<String, bool> = BTreeMap::new();

    for face in db.faces() {
        for (fam_name, _) in &face.families {
            let trimmed = fam_name.trim();
            if trimmed.is_empty() {
                continue;
            }

            let lower = trimmed.to_lowercase();
            let is_mono = face.monospaced
                || lower.contains("mono")
                || lower.contains("code")
                || lower.contains("typewriter")
                || lower.contains("console")
                || lower.contains("fixed")
                || lower.contains("courier");

            families
                .entry(trimmed.to_string())
                .and_modify(|m| *m = *m || is_mono)
                .or_insert(is_mono);
        }
    }

    let mut result: Vec<FontFamilyInfo> = families
        .into_iter()
        .map(|(name, is_monospaced)| FontFamilyInfo {
            name,
            is_monospaced,
        })
        .collect();

    // Sort alphabetically (case-insensitive)
    result.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    result
}

/// Calculates a fuzzy match score between `pattern` and `target`.
/// Higher score means a better match. Returns `None` if the pattern does not match.
pub fn fuzzy_match(pattern: &str, target: &str) -> Option<i32> {
    let p_clean = pattern.trim().to_lowercase();
    let t_clean = target.trim().to_lowercase();

    if p_clean.is_empty() {
        return Some(0);
    }

    // Exact match bonus
    if p_clean == t_clean {
        return Some(2000);
    }

    // Exact prefix match bonus
    if t_clean.starts_with(&p_clean) {
        return Some(1000 + (100 - t_clean.len() as i32));
    }

    // Exact substring match bonus
    if let Some(pos) = t_clean.find(&p_clean) {
        return Some(500 - pos as i32 * 2 - t_clean.len() as i32);
    }

    // Sequential character subsequence matching with scoring
    let p_chars: Vec<char> = p_clean.chars().collect();
    let t_chars: Vec<char> = t_clean.chars().collect();

    let mut p_idx = 0;
    let mut score = 0;
    let mut consecutive = 0;
    let mut prev_t_idx = None;

    for (t_idx, &tc) in t_chars.iter().enumerate() {
        if p_idx < p_chars.len() && tc == p_chars[p_idx] {
            // Match found for this pattern character
            let is_word_boundary = t_idx == 0
                || t_chars[t_idx - 1] == ' '
                || t_chars[t_idx - 1] == '-'
                || t_chars[t_idx - 1] == '_';

            if is_word_boundary {
                score += 30;
            }

            if let Some(prev) = prev_t_idx {
                if t_idx == prev + 1 {
                    consecutive += 1;
                    score += 15 * consecutive;
                } else {
                    consecutive = 0;
                }
            } else {
                consecutive = 1;
                score += 10;
            }

            prev_t_idx = Some(t_idx);
            p_idx += 1;
        }
    }

    if p_idx == p_chars.len() {
        // Matched all characters; slightly penalize longer target names
        score -= t_clean.len() as i32;
        Some(score)
    } else {
        None
    }
}

/// Filters and ranks a list of font families based on a fuzzy search query and monospaced filter.
pub fn filter_fonts<'a>(
    fonts: &'a [FontFamilyInfo],
    query: &str,
    only_monospaced: bool,
) -> Vec<&'a FontFamilyInfo> {
    let mut matched: Vec<(i32, &'a FontFamilyInfo)> = fonts
        .iter()
        .filter(|f| !only_monospaced || f.is_monospaced)
        .filter_map(|f| {
            if query.trim().is_empty() {
                Some((0, f))
            } else {
                fuzzy_match(query, &f.name).map(|score| (score, f))
            }
        })
        .collect();

    if !query.trim().is_empty() {
        // Sort descending by score, then alphabetically
        matched.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.name.to_lowercase().cmp(&b.1.name.to_lowercase()))
        });
    }

    matched.into_iter().map(|(_, f)| f).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_match_behavior() {
        assert!(fuzzy_match("fircod", "Fira Code").is_some());
        assert!(fuzzy_match("noto mono", "Noto Sans Mono").is_some());
        assert!(fuzzy_match("xyz", "Fira Code").is_none());

        let score_prefix = fuzzy_match("fira", "Fira Code").unwrap();
        let score_mid = fuzzy_match("code", "Fira Code").unwrap();
        assert!(score_prefix > score_mid);
    }

    #[test]
    fn test_system_fonts_query() {
        let fonts = query_system_fonts();
        assert!(!fonts.is_empty(), "Expected at least one font family on system");
        
        let mono_count = fonts.iter().filter(|f| f.is_monospaced).count();
        assert!(mono_count > 0, "Expected at least one monospace font on system");
    }
}
