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

    /// The accepted wire values, in schema order. The MCP tool's `enum` is built from
    /// this so the schema a reviewer is shown cannot drift from what parsing accepts.
    pub const WIRE: [&'static str; 3] = ["critical", "important", "nit"];
}

/// A review's overall call. Closed, because the MCP tool advertises it as a closed
/// enum and validates against it: a reviewer that submits `"bogus"` must be told so
/// while it is still running, not have the value merged and rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Clean,
    Changes,
}

impl Verdict {
    pub const WIRE: [&'static str; 2] = ["clean", "changes"];
}

/// How much of the change the findings bear on. Closed for the same reason as
/// [`Verdict`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Impact {
    Low,
    Medium,
    High,
}

impl Impact {
    pub const WIRE: [&'static str; 3] = ["low", "medium", "high"];
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Finding {
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub severity: Severity,
    /// Stamped by [`merge_reviews`] from the source file's angle.
    ///
    /// Skipped when empty, which is exactly the per-angle file a reviewer produces: the
    /// angle lives in the filename there, and writing `"angle": ""` beside it would be
    /// a second, disagreeing copy of the one thing the merge is careful to own. In the
    /// merged review it is always stamped, so it is always written.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub angle: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Review {
    pub verdict: Verdict,
    #[serde(default)]
    pub findings: Vec<Finding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impact: Option<Impact>,
}

/// Parse one reviewer's `<task>-review-<iter>-<angle>.json`.
pub fn parse_review(json: &str) -> io::Result<Review> {
    serde_json::from_str(json).map_err(io::Error::other)
}

/// Union all per-angle findings; STAMP each finding's `angle` from its source tuple (trust
/// the filename's angle over any self-reported one). The merged `verdict` is ALWAYS
/// RECOMPUTED from the merged findings (`"changes"` if any `.severity.blocks()`, else
/// `"clean"`) — per-angle `Review.verdict` fields are IGNORED. `impact` = the first
/// per-angle impact that is present.
pub fn merge_reviews(per_angle: Vec<(String, Review)>) -> Review {
    let mut findings = Vec::new();
    let mut impact: Option<Impact> = None;

    for (angle, review) in per_angle {
        if impact.is_none() {
            impact = review.impact;
        }
        for mut f in review.findings {
            f.angle = angle.clone();
            findings.push(f);
        }
    }

    let verdict = if findings.iter().any(|f| f.severity.blocks()) {
        Verdict::Changes
    } else {
        Verdict::Clean
    };

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
        assert_eq!(r.verdict, Verdict::Changes);
        assert_eq!(r.findings.len(), 1);
        assert_eq!(r.findings[0].line, Some(42));
        assert_eq!(r.findings[0].severity, Severity::Critical);
        assert_eq!(r.impact, Some(Impact::High));

        // Minimal form: no line, no impact, no rationale.
        let minimal = r#"{
            "verdict": "clean",
            "findings": [
                {"file": "a.rs", "severity": "nit", "summary": "prefer let-else"}
            ]
        }"#;
        let r = parse_review(minimal).unwrap();
        assert_eq!(r.verdict, Verdict::Clean);
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
            verdict: Verdict::Changes,
            findings: vec![
                finding("a.rs", Severity::Important, "bogus"),
                finding("b.rs", Severity::Nit, "bogus"),
            ],
            impact: None,
        };
        let security = Review {
            verdict: Verdict::Clean,
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
    fn merge_impact_is_the_first_one_present() {
        let a = Review {
            verdict: Verdict::Clean,
            findings: vec![],
            impact: None, // absent → skipped
        };
        let b = Review {
            verdict: Verdict::Clean,
            findings: vec![],
            impact: Some(Impact::Medium),
        };
        let c = Review {
            verdict: Verdict::Clean,
            findings: vec![],
            impact: Some(Impact::Low),
        };
        let merged = merge_reviews(vec![("x".into(), a), ("y".into(), b), ("z".into(), c)]);
        assert_eq!(merged.impact, Some(Impact::Medium));
    }

    /// `verdict` and `impact` are advertised to reviewers as closed enums by the MCP
    /// tool's schema. Parsing must enforce exactly that set, so a reviewer that invents
    /// a value is told while it can still correct itself — rather than having the value
    /// merged and rendered as if it meant something.
    #[test]
    fn parse_review_rejects_verdicts_and_impacts_outside_the_advertised_enums() {
        assert!(parse_review(r#"{"verdict":"bogus","findings":[]}"#).is_err());
        assert!(parse_review(r#"{"verdict":"","findings":[]}"#).is_err());
        assert!(parse_review(r#"{"verdict":"clean","impact":"critical"}"#).is_err());
        assert!(parse_review(r#"{"verdict":"clean","impact":""}"#).is_err());
        // …and accepts every value the schema does advertise.
        for v in Verdict::WIRE {
            assert!(
                parse_review(&format!(r#"{{"verdict":"{v}"}}"#)).is_ok(),
                "the schema advertises verdict '{v}'"
            );
        }
        for i in Impact::WIRE {
            assert!(
                parse_review(&format!(r#"{{"verdict":"clean","impact":"{i}"}}"#)).is_ok(),
                "the schema advertises impact '{i}'"
            );
        }
    }

    /// The web UI reads these files as raw JSON and matches on the string, so the
    /// enums must keep serializing to exactly the wire values they replaced.
    #[test]
    fn verdict_and_impact_serialize_to_their_wire_values() {
        let r = Review {
            verdict: Verdict::Changes,
            findings: vec![],
            impact: Some(Impact::High),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains(r#""verdict":"changes""#), "{json}");
        assert!(json.contains(r#""impact":"high""#), "{json}");
        assert_eq!(parse_review(&json).unwrap(), r);
    }

    #[test]
    fn clean_gate_nits_only_is_clean() {
        let r = Review {
            verdict: Verdict::Changes, // stale; is_clean looks at findings, not verdict
            findings: vec![finding("a.rs", Severity::Nit, "correctness")],
            impact: None,
        };
        assert!(is_clean(&r));
    }

    #[test]
    fn clean_gate_important_or_critical_is_not_clean() {
        let imp = Review {
            verdict: Verdict::Clean,
            findings: vec![finding("a.rs", Severity::Important, "correctness")],
            impact: None,
        };
        assert!(!is_clean(&imp));

        let crit = Review {
            verdict: Verdict::Clean,
            findings: vec![finding("a.rs", Severity::Critical, "correctness")],
            impact: None,
        };
        assert!(!is_clean(&crit));
    }

    #[test]
    fn clean_gate_empty_is_clean() {
        let r = Review {
            verdict: Verdict::Changes,
            findings: vec![],
            impact: None,
        };
        assert!(is_clean(&r));
    }

    #[test]
    fn merge_recomputes_verdict_ignoring_per_angle_verdict() {
        // verdict "changes" but zero findings → merges to "clean".
        let empty_but_changes = Review {
            verdict: Verdict::Changes,
            findings: vec![],
            impact: None,
        };
        let merged = merge_reviews(vec![("correctness".into(), empty_but_changes)]);
        assert_eq!(merged.verdict, Verdict::Clean);
        assert!(is_clean(&merged));

        // verdict "clean" but an important finding → merges to "changes".
        let clean_but_important = Review {
            verdict: Verdict::Clean,
            findings: vec![finding("a.rs", Severity::Important, "security")],
            impact: None,
        };
        let merged = merge_reviews(vec![("security".into(), clean_but_important)]);
        assert_eq!(merged.verdict, Verdict::Changes);
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
