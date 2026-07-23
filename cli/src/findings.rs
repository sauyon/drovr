//! Findings schema + parse + merge + clean gate.
//!
//! Pure data types and logic for per-angle review findings, union-merge with angle
//! tagging, and the clean gate. No orchestration and no IO beyond string parse — the
//! panel (Task 5) reads/writes the files and calls into here.
//!
//! # Merge semantics
//!
//! [`merge_reviews`] is a **pure union** (no LLM de-dup): a finding reported by two angles
//! appears twice, each copy carrying its own angle tag. Each finding's `angle` is stamped
//! from the source tuple (the filename's angle), which is trusted over any self-reported
//! `angle` in the JSON. The merged `verdict` is **always recomputed** from the merged
//! findings (`"changes"` if any finding [`Severity::blocks`], else `"clean"`) — per-angle
//! `verdict` fields are ignored, since a reviewer may set a verdict inconsistent with its
//! own findings.

use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    Important,
    Nit,
}

impl Severity {
    /// True for `Critical`|`Important` — the blocking set that fails the clean gate.
    pub fn blocks(self) -> bool {
        matches!(self, Severity::Critical | Severity::Important)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Finding {
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub severity: Severity,
    /// Stamped by [`merge_reviews`] from the source file's angle.
    #[serde(default)]
    pub angle: String,
    pub summary: String,
    #[serde(default)]
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Review {
    /// `"clean"` | `"changes"`.
    pub verdict: String,
    #[serde(default)]
    pub findings: Vec<Finding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact: Option<String>,
}

/// Parse one reviewer's `<task>-review-<angle>.json`.
pub fn parse_review(json: &str) -> io::Result<Review> {
    serde_json::from_str(json).map_err(io::Error::other)
}

/// Union all per-angle findings; STAMP each finding's `angle` from its source tuple (trust
/// the filename's angle over any self-reported one). The merged `verdict` is ALWAYS
/// RECOMPUTED from the merged findings (`"changes"` if any `.severity.blocks()`, else
/// `"clean"`) — per-angle `Review.verdict` fields are IGNORED. `impact` = first non-empty
/// per-angle impact.
pub fn merge_reviews(per_angle: Vec<(String, Review)>) -> Review {
    let mut findings = Vec::new();
    let mut impact: Option<String> = None;

    for (angle, review) in per_angle {
        if impact.is_none() {
            if let Some(i) = review.impact {
                if !i.is_empty() {
                    impact = Some(i);
                }
            }
        }
        for mut f in review.findings {
            f.angle = angle.clone();
            findings.push(f);
        }
    }

    let verdict = if findings.iter().any(|f| f.severity.blocks()) {
        "changes"
    } else {
        "clean"
    }
    .to_string();

    Review {
        verdict,
        findings,
        impact,
    }
}

/// No blocking (`Critical`|`Important`) findings.
pub fn is_clean(r: &Review) -> bool {
    !r.findings.iter().any(|f| f.severity.blocks())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_blocks_only_critical_and_important() {
        assert!(Severity::Critical.blocks());
        assert!(Severity::Important.blocks());
        assert!(!Severity::Nit.blocks());
    }

    #[test]
    fn parse_review_accepts_schema_with_optional_fields() {
        // Full form: line + impact present.
        let full = r#"{
            "verdict": "changes",
            "findings": [
                {
                    "file": "cli/src/main.rs",
                    "line": 42,
                    "severity": "critical",
                    "angle": "correctness",
                    "summary": "off-by-one",
                    "rationale": "loop overruns the buffer"
                }
            ],
            "impact": "high"
        }"#;
        let r = parse_review(full).unwrap();
        assert_eq!(r.verdict, "changes");
        assert_eq!(r.findings.len(), 1);
        assert_eq!(r.findings[0].line, Some(42));
        assert_eq!(r.findings[0].severity, Severity::Critical);
        assert_eq!(r.impact.as_deref(), Some("high"));

        // Minimal form: no line, no impact, no rationale.
        let minimal = r#"{
            "verdict": "clean",
            "findings": [
                {"file": "a.rs", "severity": "nit", "summary": "prefer let-else"}
            ]
        }"#;
        let r = parse_review(minimal).unwrap();
        assert_eq!(r.verdict, "clean");
        assert_eq!(r.findings[0].line, None);
        assert_eq!(r.findings[0].rationale, "");
        assert_eq!(r.impact, None);
    }

    #[test]
    fn parse_review_rejects_malformed_json() {
        assert!(parse_review("{ not json").is_err());
        // Unknown severity value.
        assert!(parse_review(r#"{"verdict":"clean","findings":[{"file":"a","severity":"blocker","summary":"x"}]}"#).is_err());
    }

    fn finding(file: &str, severity: Severity, angle: &str) -> Finding {
        Finding {
            file: file.into(),
            line: None,
            severity,
            angle: angle.into(),
            summary: "s".into(),
            rationale: String::new(),
        }
    }

    #[test]
    fn merge_unions_findings_and_stamps_angle_from_source_tuple() {
        // Each input finding carries a WRONG self-reported angle; merge must overwrite it
        // with the source tuple's angle.
        let correctness = Review {
            verdict: "changes".into(),
            findings: vec![
                finding("a.rs", Severity::Important, "bogus"),
                finding("b.rs", Severity::Nit, "bogus"),
            ],
            impact: None,
        };
        let security = Review {
            verdict: "clean".into(),
            findings: vec![finding("a.rs", Severity::Critical, "also-bogus")],
            impact: None,
        };

        let merged = merge_reviews(vec![
            ("correctness".into(), correctness),
            ("security".into(), security),
        ]);

        assert_eq!(merged.findings.len(), 3); // count == sum, no de-dup
        assert_eq!(merged.findings[0].angle, "correctness");
        assert_eq!(merged.findings[1].angle, "correctness");
        assert_eq!(merged.findings[2].angle, "security");
    }

    #[test]
    fn merge_impact_is_first_non_empty() {
        let a = Review {
            verdict: "clean".into(),
            findings: vec![],
            impact: Some(String::new()), // empty → skipped
        };
        let b = Review {
            verdict: "clean".into(),
            findings: vec![],
            impact: Some("medium".into()),
        };
        let c = Review {
            verdict: "clean".into(),
            findings: vec![],
            impact: Some("low".into()),
        };
        let merged = merge_reviews(vec![
            ("x".into(), a),
            ("y".into(), b),
            ("z".into(), c),
        ]);
        assert_eq!(merged.impact.as_deref(), Some("medium"));
    }

    #[test]
    fn clean_gate_nits_only_is_clean() {
        let r = Review {
            verdict: "changes".into(), // stale; is_clean looks at findings, not verdict
            findings: vec![finding("a.rs", Severity::Nit, "correctness")],
            impact: None,
        };
        assert!(is_clean(&r));
    }

    #[test]
    fn clean_gate_important_or_critical_is_not_clean() {
        let imp = Review {
            verdict: "clean".into(),
            findings: vec![finding("a.rs", Severity::Important, "correctness")],
            impact: None,
        };
        assert!(!is_clean(&imp));

        let crit = Review {
            verdict: "clean".into(),
            findings: vec![finding("a.rs", Severity::Critical, "correctness")],
            impact: None,
        };
        assert!(!is_clean(&crit));
    }

    #[test]
    fn clean_gate_empty_is_clean() {
        let r = Review {
            verdict: "changes".into(),
            findings: vec![],
            impact: None,
        };
        assert!(is_clean(&r));
    }

    #[test]
    fn merge_recomputes_verdict_ignoring_per_angle_verdict() {
        // verdict "changes" but zero findings → merges to "clean".
        let empty_but_changes = Review {
            verdict: "changes".into(),
            findings: vec![],
            impact: None,
        };
        let merged = merge_reviews(vec![("correctness".into(), empty_but_changes)]);
        assert_eq!(merged.verdict, "clean");
        assert!(is_clean(&merged));

        // verdict "clean" but an important finding → merges to "changes".
        let clean_but_important = Review {
            verdict: "clean".into(),
            findings: vec![finding("a.rs", Severity::Important, "security")],
            impact: None,
        };
        let merged = merge_reviews(vec![("security".into(), clean_but_important)]);
        assert_eq!(merged.verdict, "changes");
        assert!(!is_clean(&merged));
    }

    #[test]
    fn severity_serializes_lowercase_roundtrip() {
        // Guards the serde(rename_all="lowercase") contract the reviewer JSON depends on.
        let f = finding("a.rs", Severity::Critical, "correctness");
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"severity\":\"critical\""));
        // line is None → skipped in output.
        assert!(!json.contains("\"line\""));
        let back: Finding = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
    }
}
