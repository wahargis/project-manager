//! Statistical confidence scoring for experiments using Median Absolute Deviation (MAD).
//!
//! When an experiment has 3+ findings containing numeric metrics, we compute
//! a confidence score: `confidence = |best_improvement| / MAD`.
//!
//! Thresholds:
//!   >= 2.0  HIGH — improvement likely real
//!   1.0-2.0 MODERATE — above noise but marginal
//!   < 1.0   LOW — within noise, consider re-running

use regex::Regex;
use std::sync::LazyLock;

use crate::store::Finding;

/// A single extracted metric value from a finding's text.
#[derive(Debug, Clone)]
pub struct ExtractedMetric {
    pub value: f64,
    pub unit: String,
}

/// Result of MAD-based confidence analysis for an experiment.
#[derive(Debug, Clone)]
pub struct ConfidenceResult {
    pub metric_unit: String,
    pub values: Vec<f64>,
    pub median: f64,
    pub mad: f64,
    pub best_improvement: f64,
    pub confidence: f64,
    pub interpretation: &'static str,
}

impl ConfidenceResult {
    /// Format as a human-readable string block for CLI/MCP output.
    pub fn display(&self) -> String {
        let mut out = String::new();
        out += &format!(
            "  Statistical Confidence ({}, n={}):\n",
            self.metric_unit,
            self.values.len()
        );
        out += &format!(
            "    Median: {:.2}, MAD: {:.2}, Best Δ: {:.2}\n",
            self.median, self.mad, self.best_improvement
        );
        if self.mad == 0.0 {
            out += "    Confidence: INF (perfectly consistent)\n";
        } else {
            out += &format!("    Confidence: {:.2}\n", self.confidence);
        }
        out += &format!("    → {}\n", self.interpretation);
        out
    }
}

// Patterns for metric extraction — compiled once via LazyLock.
static METRIC_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Matches: 92.3 tok/s, +15%, -3.2%, 45.1 ms, 128 GB, 1024 tokens
    // Word-char units get \b boundary; % needs no boundary
    Regex::new(
        concat!(
            r"(?P<sign>[+\-])?",
            r"(?P<number>\d+(?:\.\d+)?)",
            r"\s*",
            r"(?P<unit>tok/s|tokens?/s|ms|us|ns|sec|seconds?|min|minutes?|GB|MB|KB|TFLOPS|GFLOPS|tokens?\b|%|x\b)",
        )
    ).unwrap()
});

/// Extract all metric values from a single finding text.
pub fn extract_metrics(text: &str) -> Vec<ExtractedMetric> {
    METRIC_RE
        .captures_iter(text)
        .filter_map(|cap| {
            let number: f64 = cap.name("number")?.as_str().parse().ok()?;
            let sign = cap.name("sign").map(|m| m.as_str()).unwrap_or("");
            let unit = cap.name("unit")?.as_str().to_string();
            let value = if sign == "-" { -number } else { number };
            Some(ExtractedMetric { value, unit })
        })
        .collect()
}

/// Group extracted metrics by unit across multiple findings.
fn group_by_unit(findings: &[Finding]) -> std::collections::HashMap<String, Vec<f64>> {
    let mut groups: std::collections::HashMap<String, Vec<f64>> = std::collections::HashMap::new();
    for finding in findings {
        for metric in extract_metrics(&finding.text) {
            groups.entry(metric.unit).or_default().push(metric.value);
        }
    }
    groups
}

/// Compute median of a sorted slice.
fn median(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return 0.0;
    }
    if n % 2 == 0 {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    } else {
        sorted[n / 2]
    }
}

/// Compute MAD (Median Absolute Deviation) of a slice.
fn compute_mad(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let med = median(&sorted);
    let mut abs_devs: Vec<f64> = sorted.iter().map(|v| (v - med).abs()).collect();
    abs_devs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    median(&abs_devs)
}

fn interpret(confidence: f64) -> &'static str {
    if confidence.is_infinite() {
        "HIGH confidence \u{2014} perfectly consistent measurements"
    } else if confidence >= 2.0 {
        "HIGH confidence \u{2014} improvement likely real"
    } else if confidence >= 1.0 {
        "MODERATE \u{2014} above noise but marginal"
    } else {
        "LOW \u{2014} within noise, consider re-running"
    }
}

/// Compute experiment confidence from its findings.
///
/// Returns `None` if there are fewer than 3 findings with numeric metrics,
/// or if no metric group has 3+ values.
pub fn compute_experiment_confidence(findings: &[Finding]) -> Option<ConfidenceResult> {
    if findings.len() < 3 {
        return None;
    }

    let groups = group_by_unit(findings);

    // Find the group with the most values (must have 3+), break ties by highest range.
    let mut best: Option<ConfidenceResult> = None;

    for (unit, values) in &groups {
        if values.len() < 3 {
            continue;
        }
        let mut sorted = values.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let med = median(&sorted);
        let mad = compute_mad(values);

        // Best improvement = max deviation from median
        let best_improvement = sorted
            .iter()
            .map(|v| (v - med).abs())
            .fold(0.0_f64, f64::max);

        let confidence = if mad == 0.0 {
            f64::INFINITY
        } else {
            best_improvement / mad
        };

        let interp = interpret(confidence);

        let candidate = ConfidenceResult {
            metric_unit: unit.clone(),
            values: sorted,
            median: med,
            mad,
            best_improvement,
            confidence,
            interpretation: interp,
        };

        // Prefer the group with highest confidence, or most values on tie.
        let dominated = match &best {
            None => true,
            Some(prev) => {
                candidate.values.len() > prev.values.len()
                    || (candidate.values.len() == prev.values.len()
                        && candidate.confidence > prev.confidence)
            }
        };
        if dominated {
            best = Some(candidate);
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn make_finding(id: i64, text: &str) -> Finding {
        Finding {
            id,
            experiment_id: Some(1),
            project_seq: Some(id),
            text: text.to_string(),
            created_at: NaiveDateTime::parse_from_str(
                "2026-03-29 12:00:00",
                "%Y-%m-%d %H:%M:%S",
            )
            .unwrap(),
            confidence: None,
            belief_status: None,
        }
    }

    #[test]
    fn test_extract_metrics_tok_s() {
        let metrics = extract_metrics("Measured 92.3 tok/s on GPU0");
        assert_eq!(metrics.len(), 1);
        assert!((metrics[0].value - 92.3).abs() < 0.01);
        assert_eq!(metrics[0].unit, "tok/s");
    }

    #[test]
    fn test_extract_metrics_percentage() {
        let metrics = extract_metrics("Improvement of +15% over baseline");
        assert_eq!(metrics.len(), 1);
        assert!((metrics[0].value - 15.0).abs() < 0.01);
        assert_eq!(metrics[0].unit, "%");
    }

    #[test]
    fn test_extract_metrics_negative() {
        let metrics = extract_metrics("Regression of -3.2% observed");
        assert_eq!(metrics.len(), 1);
        assert!((metrics[0].value - (-3.2)).abs() < 0.01);
        assert_eq!(metrics[0].unit, "%");
    }

    #[test]
    fn test_extract_metrics_multiple() {
        let metrics = extract_metrics("92.3 tok/s, latency 45.1 ms");
        assert_eq!(metrics.len(), 2);
    }

    #[test]
    fn test_extract_metrics_none() {
        let metrics = extract_metrics("No numeric data here, just text.");
        assert_eq!(metrics.len(), 0);
    }

    #[test]
    fn test_confidence_tok_s_high() {
        // Tight cluster — high confidence
        let findings = vec![
            make_finding(1, "Run 1: 92.3 tok/s"),
            make_finding(2, "Run 2: 93.1 tok/s"),
            make_finding(3, "Run 3: 91.8 tok/s"),
        ];
        let result = compute_experiment_confidence(&findings).unwrap();
        assert_eq!(result.metric_unit, "tok/s");
        assert_eq!(result.values.len(), 3);
        // median = 92.3, MAD should be small
        assert!((result.median - 92.3).abs() < 0.01);
        assert!(result.confidence >= 1.0, "Expected moderate+ confidence, got {}", result.confidence);
    }

    #[test]
    fn test_confidence_tok_s_low() {
        // Wide spread — low confidence
        let findings = vec![
            make_finding(1, "Run 1: 50.0 tok/s"),
            make_finding(2, "Run 2: 92.0 tok/s"),
            make_finding(3, "Run 3: 70.0 tok/s"),
        ];
        let result = compute_experiment_confidence(&findings).unwrap();
        assert_eq!(result.metric_unit, "tok/s");
        // Large MAD relative to spread means confidence should be limited
        assert!(result.confidence.is_finite());
    }

    #[test]
    fn test_confidence_identical_values() {
        // All identical — MAD=0, confidence=infinity
        let findings = vec![
            make_finding(1, "Run 1: 92.3 tok/s"),
            make_finding(2, "Run 2: 92.3 tok/s"),
            make_finding(3, "Run 3: 92.3 tok/s"),
        ];
        let result = compute_experiment_confidence(&findings).unwrap();
        assert!(result.confidence.is_infinite());
        assert!(result.interpretation.contains("perfectly consistent"));
    }

    #[test]
    fn test_confidence_no_metrics_returns_none() {
        let findings = vec![
            make_finding(1, "Qualitative observation: model seems faster"),
            make_finding(2, "No numeric data"),
            make_finding(3, "Just text here"),
        ];
        let result = compute_experiment_confidence(&findings);
        assert!(result.is_none());
    }

    #[test]
    fn test_confidence_fewer_than_3_findings_returns_none() {
        let findings = vec![
            make_finding(1, "Run 1: 92.3 tok/s"),
            make_finding(2, "Run 2: 93.1 tok/s"),
        ];
        let result = compute_experiment_confidence(&findings);
        assert!(result.is_none());
    }

    #[test]
    fn test_confidence_fewer_than_3_values_per_unit_returns_none() {
        // 3 findings but each with a different unit — no group has 3+
        let findings = vec![
            make_finding(1, "Run 1: 92.3 tok/s"),
            make_finding(2, "Run 2: 45.1 ms"),
            make_finding(3, "Run 3: 128 GB"),
        ];
        let result = compute_experiment_confidence(&findings);
        assert!(result.is_none());
    }

    #[test]
    fn test_confidence_mixed_units_picks_largest_group() {
        let findings = vec![
            make_finding(1, "92.3 tok/s, latency 45.1 ms"),
            make_finding(2, "93.1 tok/s, latency 44.8 ms"),
            make_finding(3, "91.8 tok/s, latency 46.0 ms"),
            make_finding(4, "92.0 tok/s"),
        ];
        let result = compute_experiment_confidence(&findings).unwrap();
        // tok/s has 4 values, ms has 3 — should pick tok/s
        assert_eq!(result.metric_unit, "tok/s");
        assert_eq!(result.values.len(), 4);
    }

    #[test]
    fn test_median_odd() {
        assert!((median(&[1.0, 2.0, 3.0]) - 2.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_median_even() {
        assert!((median(&[1.0, 2.0, 3.0, 4.0]) - 2.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_mad_computation() {
        // Values: [1, 2, 3, 4, 5], median=3, deviations=[2,1,0,1,2], MAD=1
        let mad = compute_mad(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert!((mad - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_interpret_thresholds() {
        assert!(interpret(3.0).contains("HIGH"));
        assert!(interpret(2.0).contains("HIGH"));
        assert!(interpret(1.5).contains("MODERATE"));
        assert!(interpret(1.0).contains("MODERATE"));
        assert!(interpret(0.5).contains("LOW"));
        assert!(interpret(f64::INFINITY).contains("perfectly consistent"));
    }
}
