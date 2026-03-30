//! Active contradiction detection for the PM knowledge graph.
//!
//! Two-layer cascade (ALICE architecture, Lit #38):
//! Layer 1 (this module, Rust, ms): Graph-based candidate retrieval +
//!   weighted signal detection (negation, antonyms, numerics, markers).
//! Layer 2 (Claude Code subagent, seconds): Typed CoT NLI classification
//!   on candidates from Layer 1.
//!
//! The Rust layer's job is HIGH RECALL candidate retrieval — find ALL
//! potentially contradicting nodes. False positives are cheap (subagent
//! handles precision). False negatives are permanent losses.

use std::collections::HashSet;
use crate::util::truncate_safe;

/// A detected potential contradiction candidate from Layer 1.
#[derive(Debug, Clone)]
pub struct ContradictionCandidate {
    pub node_type: String,
    pub node_id: i64,
    pub text_excerpt: String,
    pub signal_score: f64,
    pub signals: Vec<String>, // which signals fired
}

/// Result of Layer 1 contradiction scan.
#[derive(Debug)]
pub struct Layer1Result {
    pub candidates: Vec<ContradictionCandidate>,
    pub subagent_prompt: Option<String>, // pre-formatted NLI prompt for Layer 2
}

// ── Signal Detectors ──────────────────────────────────────────────

/// Negation tokens that flip claim polarity.
const NEGATION_TOKENS: &[&str] = &[
    "not", "no", "never", "neither", "nor", "none", "cannot",
    "doesn't", "don't", "didn't", "isn't", "aren't", "wasn't",
    "weren't", "won't", "wouldn't", "couldn't", "shouldn't",
    "without", "unable", "failed", "impossible", "negligible",
];

/// Explicit contradiction markers — phrases that signal self-correction.
const CONTRADICTION_MARKERS: &[&str] = &[
    "actually", "contrary to", "however", "but in fact",
    "correction:", "update:", "was wrong", "turns out",
    "not the case", "contradicts", "invalidates", "disproves",
    "previously thought", "earlier finding", "revised understanding",
];

/// Domain-specific antonym pairs relevant to GPU kernel optimization.
const ANTONYM_PAIRS: &[(&str, &str)] = &[
    ("increase", "decrease"), ("faster", "slower"), ("improve", "degrade"),
    ("saturated", "underutilized"), ("bottleneck", "sufficient"),
    ("bound", "free"), ("limited", "unlimited"), ("active", "idle"),
    ("dominated", "balanced"), ("feasible", "infeasible"),
    ("optimal", "suboptimal"), ("efficient", "inefficient"),
    ("coalesced", "scattered"), ("contiguous", "fragmented"),
    ("success", "failure"), ("pass", "fail"), ("works", "broken"),
    ("correct", "incorrect"), ("valid", "invalid"),
    ("higher", "lower"), ("more", "less"), ("above", "below"),
];

/// Count negation tokens in text. Odd count = negated claim.
fn negation_parity(text: &str) -> bool {
    let lower = text.to_lowercase();
    let count = NEGATION_TOKENS.iter()
        .filter(|&&neg| {
            // Word boundary matching: check the negation is a whole word
            lower.split(|c: char| !c.is_alphanumeric() && c != '\'')
                .any(|w| w == neg)
        })
        .count();
    count % 2 == 1 // odd = negated
}

/// Check if two texts have opposite negation parity on shared topics.
fn negation_signal(text_a: &str, text_b: &str) -> (bool, f64) {
    let neg_a = negation_parity(text_a);
    let neg_b = negation_parity(text_b);
    if neg_a != neg_b {
        (true, 0.4)
    } else {
        (false, 0.0)
    }
}

/// Check if both members of any antonym pair appear across the two texts.
fn antonym_signal(text_a: &str, text_b: &str) -> (bool, f64) {
    let lower_a = text_a.to_lowercase();
    let lower_b = text_b.to_lowercase();

    for &(word1, word2) in ANTONYM_PAIRS {
        let a_has_1 = lower_a.contains(word1);
        let a_has_2 = lower_a.contains(word2);
        let b_has_1 = lower_b.contains(word1);
        let b_has_2 = lower_b.contains(word2);

        // One text has word1, other has word2 (or vice versa)
        if (a_has_1 && b_has_2) || (a_has_2 && b_has_1) {
            return (true, 0.3);
        }
    }
    (false, 0.0)
}

/// Extract numbers from text and check for divergence.
fn numeric_signal(text_a: &str, text_b: &str) -> (bool, f64) {
    let nums_a = extract_numbers_with_context(text_a);
    let nums_b = extract_numbers_with_context(text_b);

    if nums_a.is_empty() || nums_b.is_empty() {
        return (false, 0.0);
    }

    // Only compare numbers when surrounding context shares keywords.
    // "3.5 bits per element" vs "900 GB/s bandwidth" should NOT fire
    // because context words (bits/element vs GB/bandwidth) don't overlap.
    // "97.2 tok/s" vs "45.6 tok/s" SHOULD fire because "tok" matches.
    for (na, ctx_a) in &nums_a {
        for (nb, ctx_b) in &nums_b {
            let shared = ctx_a.iter().any(|wa| ctx_b.iter().any(|wb| wa == wb && wa.len() >= 3));
            if !shared { continue; }

            if *na > 0.0 && *nb > 0.0 {
                let ratio = na / nb;
                if ratio > 1.5 || ratio < 0.67 {
                    return (true, 0.5);
                }
            }
        }
    }
    (false, 0.0)
}

/// Extract numbers with surrounding context words (2 words before + after).
fn extract_numbers_with_context(text: &str) -> Vec<(f64, Vec<String>)> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut results = Vec::new();
    for (i, word) in words.iter().enumerate() {
        let cleaned: String = word.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
        if let Ok(n) = cleaned.parse::<f64>() {
            if n > 1.0 {
                let start = if i >= 2 { i - 2 } else { 0 };
                let end = std::cmp::min(i + 3, words.len());
                let ctx: Vec<String> = words[start..end].iter()
                    .map(|w| w.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_string())
                    .filter(|w| w.len() >= 2)
                    .collect();
                results.push((n, ctx));
            }
        }
    }
    results
}

/// Simple number extraction via character scanning.
fn extract_numbers(text: &str) -> Vec<f64> {
    let mut nums = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let num_str: String = chars[start..i].iter().collect();
            if let Ok(n) = num_str.parse::<f64>() {
                if n > 1.0 { // ignore single digits and tiny numbers
                    nums.push(n);
                }
            }
        } else {
            i += 1;
        }
    }
    nums
}

/// Check for explicit contradiction markers.
fn marker_signal(text_a: &str, text_b: &str) -> (bool, f64) {
    let lower_a = text_a.to_lowercase();
    let lower_b = text_b.to_lowercase();

    for marker in CONTRADICTION_MARKERS {
        if lower_a.contains(marker) || lower_b.contains(marker) {
            return (true, 0.6);
        }
    }
    (false, 0.0)
}

/// Run all signal detectors on a candidate pair.
pub fn score_pair(new_text: &str, existing_text: &str) -> (f64, Vec<String>) {
    let mut score = 0.0;
    let mut signals = Vec::new();

    let (neg, neg_s) = negation_signal(new_text, existing_text);
    if neg { score += neg_s; signals.push("negation_parity".into()); }

    let (ant, ant_s) = antonym_signal(new_text, existing_text);
    if ant { score += ant_s; signals.push("antonym_pair".into()); }

    let (num, num_s) = numeric_signal(new_text, existing_text);
    if num { score += num_s; signals.push("numeric_divergence".into()); }

    let (mrk, mrk_s) = marker_signal(new_text, existing_text);
    if mrk { score += mrk_s; signals.push("contradiction_marker".into()); }

    (score, signals)
}

// ── Layer 2 Prompt Generation ─────────────────────────────────────

/// Generate the Claude subagent NLI prompt for Layer 2 classification.
/// Based on the RAG contradiction paper: CoT + typed classification.
pub fn generate_nli_prompt(
    new_text: &str,
    candidates: &[ContradictionCandidate],
) -> String {
    let mut prompt = String::from(
        "You are a contradiction analyst for a research knowledge base.\n\
         Determine whether these research entries contain contradictory claims.\n\n\
         ## Contradiction Types\n\
         1. DIRECT_NEGATION: One entry explicitly negates a claim in the other\n\
         2. NUMERIC_CONFLICT: Entries report different numbers for the same measurement\n\
         3. TEMPORAL_SUPERSESSION: A later entry implicitly invalidates an earlier one\n\
         4. CAUSAL_CONFLICT: Entries attribute different causes to the same effect\n\
         5. RECOMMENDATION_CONFLICT: Entries recommend opposite actions\n\
         6. NONE: No contradiction exists\n\n\
         ## Important: NOT contradictions\n\
         - Different contexts (GPU vs CPU) are NOT contradictions\n\
         - Explicit updates/corrections that acknowledge the change are UPDATES not contradictions\n\
         - Different granularity (~95 tok/s vs 97.2 tok/s) is compatible\n\
         - Uncertainty vs certainty (might work vs works) is compatible\n\n"
    );

    prompt.push_str(&format!("## New Entry:\n{}\n\n", new_text));

    for (i, c) in candidates.iter().enumerate() {
        prompt.push_str(&format!(
            "## Existing Entry {} ({} #{}, signals: {}):\n{}\n\n",
            i + 1, c.node_type, c.node_id,
            c.signals.join(", "),
            c.text_excerpt
        ));
    }

    prompt.push_str(
        "## Analysis\n\
         Think step by step for each existing entry:\n\
         1. What specific claims does each entry make?\n\
         2. Do any claims directly conflict with the new entry?\n\
         3. Could the apparent conflict be explained by different contexts?\n\n\
         Respond with JSON array:\n\
         [{\"entry\": 1, \"is_contradiction\": bool, \"type\": \"...\", \
         \"confidence\": 0.0-1.0, \"explanation\": \"...\"}]\n"
    );

    prompt
}

/// Format Layer 1 results for MCP tool response.
pub fn format_layer1_results(new_text: &str, results: &Layer1Result) -> String {
    let mut out = String::new();

    if results.candidates.is_empty() {
        return out; // no candidates, nothing to report
    }

    out.push_str("\n=== Contradiction Detection (Layer 1) ===\n");
    out.push_str(&format!("{} candidate(s) flagged for review:\n\n", results.candidates.len()));

    for c in &results.candidates {
        let excerpt = if c.text_excerpt.len() > 120 {
            format!("{}...", truncate_safe(&c.text_excerpt, 120))
        } else {
            c.text_excerpt.clone()
        };
        out.push_str(&format!(
            "  {} #{} [score:{:.1}] signals: {}\n    \"{}\"\n\n",
            c.node_type, c.node_id, c.signal_score,
            c.signals.join(", "), excerpt
        ));
    }

    if let Some(ref prompt) = results.subagent_prompt {
        out.push_str("Layer 2 NLI analysis suggested. Prompt available for Claude subagent.\n");
        // The prompt is available programmatically but not dumped to output
        // (it would be too long for the MCP response)
        let _ = prompt; // suppress unused warning
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_negation_parity_positive() {
        assert!(!negation_parity("The kernel is bandwidth-bound"));
    }

    #[test]
    fn test_negation_parity_negative() {
        assert!(negation_parity("The kernel is NOT bandwidth-bound"));
    }

    #[test]
    fn test_negation_parity_double_negative() {
        assert!(!negation_parity("It is not impossible to optimize"));
    }

    #[test]
    fn test_negation_signal_opposite() {
        let (fired, score) = negation_signal(
            "The FP pipeline is saturated during decode",
            "The FP pipeline is NOT saturated during decode"
        );
        assert!(fired);
        assert!(score > 0.0);
    }

    #[test]
    fn test_antonym_signal_detected() {
        let (fired, _) = antonym_signal(
            "Performance increased with the optimization",
            "Performance decreased after the change"
        );
        assert!(fired);
    }

    #[test]
    fn test_antonym_signal_no_match() {
        let (fired, _) = antonym_signal(
            "The kernel uses shared memory",
            "The algorithm processes data in parallel"
        );
        assert!(!fired);
    }

    #[test]
    fn test_numeric_divergence() {
        let (fired, _) = numeric_signal(
            "Achieved 97.2 tok/s on the NVLink pair",
            "Measured only 45.6 tok/s on the same configuration"
        );
        assert!(fired);
    }

    #[test]
    fn test_numeric_compatible() {
        let (fired, _) = numeric_signal(
            "Achieved 97.2 tok/s",
            "Measured 95.4 tok/s" // within 1.5x ratio
        );
        assert!(!fired);
    }

    #[test]
    fn test_marker_signal() {
        let (fired, _) = marker_signal(
            "The original analysis was correct",
            "Actually, the previous finding was wrong about the bottleneck"
        );
        assert!(fired);
    }

    #[test]
    fn test_score_pair_multiple_signals() {
        let (score, signals) = score_pair(
            "The kernel is NOT bandwidth-bound, achieving only 45 tok/s",
            "The kernel is bandwidth-bound, achieving 97 tok/s"
        );
        assert!(score >= 0.9, "Multiple signals should produce high score, got {}", score);
        assert!(signals.len() >= 2, "Should fire multiple signals, got {:?}", signals);
    }

    #[test]
    fn test_score_pair_no_contradiction() {
        let (score, signals) = score_pair(
            "TurboQuant achieves 3.5 bits per element",
            "The V100 has 900 GB/s HBM2 bandwidth"
        );
        assert!(score < 0.3, "Unrelated texts should have low score, got {}", score);
        assert!(signals.is_empty());
    }

    #[test]
    fn test_generate_nli_prompt_structure() {
        let prompt = generate_nli_prompt(
            "New finding text",
            &[ContradictionCandidate {
                node_type: "Finding".into(),
                node_id: 31,
                text_excerpt: "Old finding text".into(),
                signal_score: 0.7,
                signals: vec!["negation_parity".into()],
            }]
        );
        assert!(prompt.contains("DIRECT_NEGATION"));
        assert!(prompt.contains("New finding text"));
        assert!(prompt.contains("Old finding text"));
        assert!(prompt.contains("Think step by step"));
    }
}
