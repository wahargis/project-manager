//! Input validation framework for project-manager MCP tools.
//!
//! Validates data quality before store operations. On failure, returns
//! structured errors with template guidance showing the expected format.

/// A single validation failure for a specific field.
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
    pub template: Option<String>,
}

/// Aggregated result of validating one or more fields.
#[derive(Debug)]
pub struct ValidationResult {
    pub errors: Vec<ValidationError>,
}

impl ValidationResult {
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// Format all errors into a readable MCP error response with template guidance.
    pub fn to_mcp_error(&self) -> String {
        let mut out = String::new();
        for (i, err) in self.errors.iter().enumerate() {
            out += &format!("{}. [{}] {}\n", i + 1, err.field, err.message);
        }
        // Collect unique templates
        let mut templates: Vec<&str> = Vec::new();
        for err in &self.errors {
            if let Some(ref t) = err.template {
                if !templates.contains(&t.as_str()) {
                    templates.push(t);
                }
            }
        }
        if !templates.is_empty() {
            out += "\nExpected format:\n";
            for t in templates {
                out += &format!("  {}\n", t);
            }
        }
        out
    }

    fn push(&mut self, err: Option<ValidationError>) {
        if let Some(e) = err {
            self.errors.push(e);
        }
    }
}

// --- Core Helpers ---

fn check_min_length(field: &str, value: &str, min: usize) -> Option<ValidationError> {
    let trimmed = value.trim();
    if trimmed.len() < min {
        Some(ValidationError {
            field: field.to_string(),
            message: format!("{} chars provided, minimum {} required", trimmed.len(), min),
            template: None,
        })
    } else {
        None
    }
}

fn check_required(field: &str, value: Option<&str>) -> Option<ValidationError> {
    match value {
        None | Some("") => Some(ValidationError {
            field: field.to_string(),
            message: "required but not provided".to_string(),
            template: None,
        }),
        Some(v) if v.trim().is_empty() => Some(ValidationError {
            field: field.to_string(),
            message: "required but empty/whitespace".to_string(),
            template: None,
        }),
        _ => None,
    }
}

fn check_enum(field: &str, value: &str, valid: &[&str]) -> Option<ValidationError> {
    if valid.contains(&value) {
        None
    } else {
        Some(ValidationError {
            field: field.to_string(),
            message: format!("'{}' is not valid. Options: {}", value, valid.join(", ")),
            template: None,
        })
    }
}

// --- Per-Node-Type Validators ---

const FINDING_TEMPLATE: &str = "A detailed observation (min 100 chars) including: what was observed, under what conditions, and what it implies.";

pub fn validate_finding(text: &str) -> ValidationResult {
    let mut r = ValidationResult::new();
    let mut err = check_min_length("text", text, 100);
    if let Some(ref mut e) = err {
        e.template = Some(FINDING_TEMPLATE.to_string());
    }
    r.push(err);
    r
}

const DECISION_TEMPLATE: &str = "Decision 'what': describe the decision made (min 50 chars). Decision 'why': explain the rationale, alternatives considered, and evidence (REQUIRED, min 50 chars).";

pub fn validate_decision(what: &str, why: Option<&str>) -> ValidationResult {
    let mut r = ValidationResult::new();
    let mut err = check_min_length("what", what, 50);
    if let Some(ref mut e) = err {
        e.template = Some(DECISION_TEMPLATE.to_string());
    }
    r.push(err);

    let mut req_err = check_required("why", why);
    if let Some(ref mut e) = req_err {
        e.template = Some(DECISION_TEMPLATE.to_string());
    }
    r.push(req_err);

    // If why is present, check min length
    if let Some(why_val) = why {
        if !why_val.trim().is_empty() {
            let mut len_err = check_min_length("why", why_val, 50);
            if let Some(ref mut e) = len_err {
                e.template = Some(DECISION_TEMPLATE.to_string());
            }
            r.push(len_err);
        }
    }
    r
}

const LITERATURE_TEMPLATE: &str = "Literature entry requires: title (required), authors (REQUIRED), at least one of arxiv_id or url, key_findings (min 200 chars), relevance (min 100 chars). Example: title='Attention Is All You Need', authors='Vaswani et al.', arxiv_id='1706.03762', key_findings='Introduced the Transformer architecture...', relevance='Foundational for our attention optimization work...'";

pub fn validate_literature(
    title: &str,
    authors: Option<&str>,
    arxiv_id: Option<&str>,
    url: Option<&str>,
    key_findings: Option<&str>,
    relevance: Option<&str>,
) -> ValidationResult {
    let mut r = ValidationResult::new();

    // title required
    let mut err = check_required("title", Some(title));
    if let Some(ref mut e) = err {
        e.template = Some(LITERATURE_TEMPLATE.to_string());
    }
    r.push(err);

    // authors REQUIRED
    let mut err = check_required("authors", authors);
    if let Some(ref mut e) = err {
        e.template = Some(LITERATURE_TEMPLATE.to_string());
    }
    r.push(err);

    // at least one of arxiv_id or url
    let has_arxiv = arxiv_id.map(|s| !s.trim().is_empty()).unwrap_or(false);
    let has_url = url.map(|s| !s.trim().is_empty()).unwrap_or(false);
    if !has_arxiv && !has_url {
        r.errors.push(ValidationError {
            field: "arxiv_id/url".to_string(),
            message: "at least one of arxiv_id or url is required".to_string(),
            template: Some(LITERATURE_TEMPLATE.to_string()),
        });
    }

    // key_findings min 200 chars
    if let Some(kf) = key_findings {
        let mut err = check_min_length("key_findings", kf, 200);
        if let Some(ref mut e) = err {
            e.template = Some(LITERATURE_TEMPLATE.to_string());
        }
        r.push(err);
    }

    // relevance min 100 chars
    if let Some(rel) = relevance {
        let mut err = check_min_length("relevance", rel, 100);
        if let Some(ref mut e) = err {
            e.template = Some(LITERATURE_TEMPLATE.to_string());
        }
        r.push(err);
    }

    r
}

const HYPOTHESIS_TEMPLATE: &str = "Hypothesis text (min 100 chars): state the hypothesis clearly. Prediction: what measurable outcome is expected. Criteria: how will this be evaluated.";

pub fn validate_hypothesis(
    text: &str,
    prediction: Option<&str>,
    criteria: Option<&str>,
) -> ValidationResult {
    let mut r = ValidationResult::new();

    let mut err = check_min_length("text", text, 100);
    if let Some(ref mut e) = err {
        e.template = Some(HYPOTHESIS_TEMPLATE.to_string());
    }
    r.push(err);

    // prediction: recommended (warn if missing, but not an error)
    if prediction.is_none() || prediction.map(|s| s.trim().is_empty()).unwrap_or(true) {
        r.errors.push(ValidationError {
            field: "prediction".to_string(),
            message: "recommended: specify a measurable predicted outcome".to_string(),
            template: Some(HYPOTHESIS_TEMPLATE.to_string()),
        });
    }

    // criteria: recommended
    if criteria.is_none() || criteria.map(|s| s.trim().is_empty()).unwrap_or(true) {
        r.errors.push(ValidationError {
            field: "criteria".to_string(),
            message: "recommended: specify evaluation criteria".to_string(),
            template: Some(HYPOTHESIS_TEMPLATE.to_string()),
        });
    }

    r
}

const PRINCIPLE_TEMPLATE: &str = "Principle text (min 50 chars): state the principle or design guideline clearly. Rationale: explain why this principle matters.";

pub fn validate_principle(text: &str, rationale: Option<&str>) -> ValidationResult {
    let mut r = ValidationResult::new();

    let mut err = check_min_length("text", text, 50);
    if let Some(ref mut e) = err {
        e.template = Some(PRINCIPLE_TEMPLATE.to_string());
    }
    r.push(err);

    // rationale: recommended
    if rationale.is_none() || rationale.map(|s| s.trim().is_empty()).unwrap_or(true) {
        r.errors.push(ValidationError {
            field: "rationale".to_string(),
            message: "recommended: explain why this principle matters".to_string(),
            template: Some(PRINCIPLE_TEMPLATE.to_string()),
        });
    }

    r
}

const CONSTRAINT_TEMPLATE: &str = "Constraint text (min 50 chars): state the constraint clearly. Source (REQUIRED): where this constraint comes from (e.g., 'hardware spec', 'user requirement', 'benchmark result').";

pub fn validate_constraint(text: &str, source: Option<&str>) -> ValidationResult {
    let mut r = ValidationResult::new();

    let mut err = check_min_length("text", text, 50);
    if let Some(ref mut e) = err {
        e.template = Some(CONSTRAINT_TEMPLATE.to_string());
    }
    r.push(err);

    let mut req_err = check_required("source", source);
    if let Some(ref mut e) = req_err {
        e.template = Some(CONSTRAINT_TEMPLATE.to_string());
    }
    r.push(req_err);

    r
}

const VALID_EDGE_RELATIONS: &[&str] = &[
    "supports", "contradicts", "depends", "informed", "supersedes",
    "related", "produced", "cited", "contains", "derived_from",
    "tested_by", "violated_by", "branches_from", "converges_into",
];

pub fn validate_edge_relation(relation: &str) -> ValidationResult {
    let mut r = ValidationResult::new();
    let err = check_enum("relation", relation, VALID_EDGE_RELATIONS);
    r.push(err);
    r
}

/// Validate status values per entity type.
pub fn validate_status(entity_type: &str, status: &str) -> ValidationResult {
    let mut r = ValidationResult::new();
    let valid = match entity_type {
        "experiment" => &["pending", "pass", "fail", "inconclusive"][..],
        "phase" => &["pending", "in_progress", "complete", "paused"][..],
        "hypothesis" => &["proposed", "testing", "confirmed", "refuted"][..],
        "research" => &["pending", "in_progress", "complete", "abandoned"][..],
        "literature" => &["unread", "read", "cited", "tested", "dead_end", "promising", "integrated"][..],
        _ => {
            r.errors.push(ValidationError {
                field: "entity_type".to_string(),
                message: format!("unknown entity type '{}'. Valid: experiment, phase, hypothesis, research, literature", entity_type),
                template: None,
            });
            return r;
        }
    };
    let err = check_enum("status", status, valid);
    r.push(err);
    r
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_finding_too_short_rejected() {
        let short = "a".repeat(50);
        let result = validate_finding(&short);
        assert!(!result.is_ok());
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].field, "text");
        assert!(result.errors[0].message.contains("50 chars"));
        assert!(result.errors[0].message.contains("100 required"));
    }

    #[test]
    fn test_finding_sufficient_accepted() {
        let enough = "a".repeat(100);
        let result = validate_finding(&enough);
        assert!(result.is_ok());
    }

    #[test]
    fn test_decision_why_required() {
        let what = "a".repeat(60);
        let result = validate_decision(&what, None);
        assert!(!result.is_ok());
        let why_errors: Vec<_> = result.errors.iter().filter(|e| e.field == "why").collect();
        assert!(!why_errors.is_empty());
        assert!(why_errors[0].message.contains("required"));
        // Verify template is present
        assert!(why_errors[0].template.is_some());
    }

    #[test]
    fn test_decision_valid() {
        let what = "a".repeat(60);
        let why = "b".repeat(60);
        let result = validate_decision(&what, Some(&why));
        assert!(result.is_ok());
    }

    #[test]
    fn test_literature_authors_required() {
        let result = validate_literature(
            "Some Paper Title",
            None, // authors missing
            Some("2301.00001"),
            None,
            Some(&"x".repeat(200)),
            Some(&"y".repeat(100)),
        );
        assert!(!result.is_ok());
        let author_errors: Vec<_> = result.errors.iter().filter(|e| e.field == "authors").collect();
        assert!(!author_errors.is_empty());
    }

    #[test]
    fn test_literature_needs_arxiv_or_url() {
        let result = validate_literature(
            "Some Paper Title",
            Some("Author A, Author B"),
            None, // no arxiv_id
            None, // no url
            Some(&"x".repeat(200)),
            Some(&"y".repeat(100)),
        );
        assert!(!result.is_ok());
        let ref_errors: Vec<_> = result.errors.iter().filter(|e| e.field == "arxiv_id/url").collect();
        assert!(!ref_errors.is_empty());
    }

    #[test]
    fn test_literature_key_findings_min_length() {
        let result = validate_literature(
            "Some Paper Title",
            Some("Author A"),
            Some("2301.00001"),
            None,
            Some(&"x".repeat(100)), // 100 chars, need 200
            Some(&"y".repeat(100)),
        );
        assert!(!result.is_ok());
        let kf_errors: Vec<_> = result.errors.iter().filter(|e| e.field == "key_findings").collect();
        assert!(!kf_errors.is_empty());
        assert!(kf_errors[0].message.contains("100 chars"));
        assert!(kf_errors[0].message.contains("200 required"));
    }

    #[test]
    fn test_invalid_status_rejected() {
        let result = validate_status("experiment", "foobar");
        assert!(!result.is_ok());
        assert!(result.errors[0].message.contains("foobar"));
        assert!(result.errors[0].message.contains("pass"));
        assert!(result.errors[0].message.contains("fail"));
    }

    #[test]
    fn test_valid_status_accepted() {
        let result = validate_status("experiment", "pass");
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_edge_relation_rejected() {
        let result = validate_edge_relation("foobar");
        assert!(!result.is_ok());
        assert!(result.errors[0].message.contains("foobar"));
        assert!(result.errors[0].message.contains("supports"));
        assert!(result.errors[0].message.contains("contradicts"));
    }

    #[test]
    fn test_valid_edge_relation_accepted() {
        let result = validate_edge_relation("supports");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validation_returns_template() {
        let result = validate_finding("too short");
        assert!(!result.is_ok());
        let mcp_error = result.to_mcp_error();
        assert!(mcp_error.contains("Expected format:"));
        assert!(mcp_error.contains("what was observed"));
    }

    // Additional edge cases

    #[test]
    fn test_decision_why_too_short() {
        let what = "a".repeat(60);
        let why = "short";
        let result = validate_decision(&what, Some(why));
        assert!(!result.is_ok());
        let why_errors: Vec<_> = result.errors.iter().filter(|e| e.field == "why").collect();
        assert!(!why_errors.is_empty());
    }

    #[test]
    fn test_constraint_source_required() {
        let text = "a".repeat(60);
        let result = validate_constraint(&text, None);
        assert!(!result.is_ok());
        let src_errors: Vec<_> = result.errors.iter().filter(|e| e.field == "source").collect();
        assert!(!src_errors.is_empty());
    }

    #[test]
    fn test_constraint_valid() {
        let text = "a".repeat(60);
        let result = validate_constraint(&text, Some("hardware spec"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_principle_text_too_short() {
        let result = validate_principle("short", None);
        assert!(!result.is_ok());
        let text_errors: Vec<_> = result.errors.iter().filter(|e| e.field == "text").collect();
        assert!(!text_errors.is_empty());
    }

    #[test]
    fn test_principle_valid_with_rationale() {
        let text = "a".repeat(60);
        let result = validate_principle(&text, Some("because it matters"));
        assert!(result.is_ok());
    }

    #[test]
    fn test_hypothesis_text_too_short() {
        let result = validate_hypothesis("short", Some("prediction"), Some("criteria"));
        assert!(!result.is_ok());
        let text_errors: Vec<_> = result.errors.iter().filter(|e| e.field == "text").collect();
        assert!(!text_errors.is_empty());
    }

    #[test]
    fn test_hypothesis_warns_no_prediction() {
        let text = "a".repeat(110);
        let result = validate_hypothesis(&text, None, Some("criteria"));
        // Not ok because prediction is recommended (warning)
        assert!(!result.is_ok());
        let pred_errors: Vec<_> = result.errors.iter().filter(|e| e.field == "prediction").collect();
        assert!(!pred_errors.is_empty());
        assert!(pred_errors[0].message.contains("recommended"));
    }

    #[test]
    fn test_all_statuses_for_experiment() {
        for s in &["pending", "pass", "fail", "inconclusive"] {
            assert!(validate_status("experiment", s).is_ok(), "experiment status '{}' should be valid", s);
        }
    }

    #[test]
    fn test_all_statuses_for_hypothesis() {
        for s in &["proposed", "testing", "confirmed", "refuted"] {
            assert!(validate_status("hypothesis", s).is_ok(), "hypothesis status '{}' should be valid", s);
        }
    }

    #[test]
    fn test_all_edge_relations() {
        for r in VALID_EDGE_RELATIONS {
            assert!(validate_edge_relation(r).is_ok(), "edge relation '{}' should be valid", r);
        }
    }

    #[test]
    fn test_literature_valid_with_url_only() {
        let result = validate_literature(
            "Blog Post Title",
            Some("Author A"),
            None, // no arxiv
            Some("https://example.com/post"),
            Some(&"x".repeat(200)),
            Some(&"y".repeat(100)),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_unknown_entity_type() {
        let result = validate_status("foobar", "pass");
        assert!(!result.is_ok());
        assert!(result.errors[0].message.contains("unknown entity type"));
    }

    #[test]
    fn test_whitespace_only_treated_as_empty() {
        let result = validate_finding("   ");
        assert!(!result.is_ok());
    }

    #[test]
    fn test_mcp_error_format_multiple_errors() {
        let result = validate_decision("short", None);
        let error = result.to_mcp_error();
        // Should have numbered errors
        assert!(error.contains("1."));
        assert!(error.contains("2."));
        assert!(error.contains("[what]"));
        assert!(error.contains("[why]"));
    }
}
