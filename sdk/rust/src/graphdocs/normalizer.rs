//! Status normalization for document titles/headers
//!
//! Normalizes variant status markers (e.g., "[Done]", "[**Done**]", "[Complete]")
//! to standard enum values.

use regex::Regex;
use serde::{Deserialize, Serialize};

/// Known status patterns and their normalized values
#[derive(Debug, Clone)]
pub struct StatusPattern {
    pub pattern: Regex,
    pub normalized: ExtendedStatus,
    pub confidence: f32,
}

/// Status variants that map to standard enum
pub const STATUS_MAPPINGS: &[(&str, &str)] = &[
    // Done variants
    ("Done", "Done"),
    ("**Done**", "Done"),
    ("[Done]", "Done"),
    ("**DONE**", "Done"),
    ("DONE", "Done"),
    ("Complete", "Done"),
    ("Completed", "Done"),
    ("✅", "Done"),
    ("Done ✅", "Done"),
    // Dev Complete -> Review (needs QA)
    ("Dev Complete", "Review"),
    ("**Dev Complete**", "Review"),
    ("Development Complete", "Review"),
    // In Progress variants
    ("In Progress", "InProgress"),
    ("WIP", "InProgress"),
    ("Working", "InProgress"),
    // Superseded/Cancelled -> special handling
    ("Superseded", "Superseded"),
    ("Cancelled", "Cancelled"),
    ("Deprecated", "Deprecated"),
    // Draft variants
    ("Draft", "Draft"),
    ("TODO", "Draft"),
    ("Planned", "Draft"),
    // Optional/Experimental markers (keep Done status but flag)
    ("Optional", "Done"),     // with optional=true flag
    ("Experimental", "Done"), // with experimental=true flag
];

/// Extended status enum to handle edge cases
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtendedStatus {
    // Standard statuses
    Draft,
    Approved,
    InProgress,
    Review,
    Done,
    // Extended statuses
    Superseded { see_also: Option<String> },
    Cancelled,
    Deprecated,
}

impl Default for ExtendedStatus {
    fn default() -> Self {
        Self::Draft
    }
}

/// Parsed status from document title/header
#[derive(Debug, Clone)]
pub struct ParsedStatus {
    pub status: ExtendedStatus,
    pub raw_text: String,
    pub optional: bool,
    pub experimental: bool,
    pub progress: Option<ProgressInfo>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProgressInfo {
    pub completed: u32,
    pub total: u32,
}

/// Extract and normalize status from document title
pub fn extract_status(title: &str) -> Option<ParsedStatus> {
    // Pattern: [STATUS] or [STATUS - notes] or [STATUS (progress)]
    let re = Regex::new(r"^\s*\[([^\]]+)\]").unwrap();

    if let Some(caps) = re.captures(title) {
        let raw = caps[1].to_string();
        let status_text = normalize_status_text(&raw);

        // Check for progress pattern like "(8/9 stories complete)"
        let progress = extract_progress(&raw);

        // Check for "See X" references
        let see_also = extract_see_also(&raw);

        // Check for optional/experimental flags
        let optional = raw.to_lowercase().contains("optional");
        let experimental = raw.to_lowercase().contains("experimental");

        // Map to extended status
        let status = map_to_status(&status_text, see_also);

        let notes = extract_notes(&raw);
        return Some(ParsedStatus {
            status,
            raw_text: raw,
            optional,
            experimental,
            progress,
            notes,
        });
    }
    None
}

fn normalize_status_text(raw: &str) -> String {
    // Remove markdown formatting
    let mut text = raw.replace("**", "");
    // Remove emoji
    text = text.replace("✅", "").replace("🔄", "").replace("⏳", "");
    // Extract first word/phrase before special chars
    if let Some(idx) = text.find(['-', '(', '|']) {
        text = text[..idx].to_string();
    }
    text.trim().to_string()
}

fn extract_progress(raw: &str) -> Option<ProgressInfo> {
    let re = Regex::new(r"\((\d+)/(\d+)").unwrap();
    re.captures(raw).map(|caps| ProgressInfo {
        completed: caps[1].parse().unwrap_or(0),
        total: caps[2].parse().unwrap_or(0),
    })
}

fn extract_see_also(raw: &str) -> Option<String> {
    // Match "See X" or "→ X" where X is the reference we want to capture
    // Handle cases like "Superseded → See TEA-001" where we want "TEA-001"
    let re = Regex::new(r"(?:See\s+|→\s*(?:See\s+)?)([\w\-\.]+)").unwrap();
    re.captures(raw).map(|caps| caps[1].to_string())
}

fn extract_notes(raw: &str) -> Option<String> {
    // Extract text after " - " that isn't a reference
    if let Some(idx) = raw.find(" - ") {
        let notes = raw[idx + 3..].trim();
        if !notes.starts_with("See") && !notes.starts_with("→") {
            return Some(notes.to_string());
        }
    }
    None
}

fn map_to_status(text: &str, see_also: Option<String>) -> ExtendedStatus {
    let lower = text.to_lowercase();

    if lower.contains("superseded") {
        return ExtendedStatus::Superseded { see_also };
    }
    if lower.contains("cancelled") || lower.contains("canceled") {
        return ExtendedStatus::Cancelled;
    }
    if lower.contains("deprecated") {
        return ExtendedStatus::Deprecated;
    }
    if lower.contains("dev complete") || lower.contains("development complete") {
        return ExtendedStatus::Review;
    }
    if lower.contains("done") || (lower.contains("complete") && !lower.contains("dev")) {
        return ExtendedStatus::Done;
    }
    if lower.contains("progress") || lower.contains("wip") {
        return ExtendedStatus::InProgress;
    }
    if lower.contains("approved") {
        return ExtendedStatus::Approved;
    }
    if lower.contains("review") {
        return ExtendedStatus::Review;
    }

    ExtendedStatus::Draft
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_normalization_done_variants() {
        assert_eq!(
            extract_status("[Done]").unwrap().status,
            ExtendedStatus::Done
        );
        assert_eq!(
            extract_status("[**Done**]").unwrap().status,
            ExtendedStatus::Done
        );
        assert_eq!(
            extract_status("[Complete]").unwrap().status,
            ExtendedStatus::Done
        );
        assert_eq!(
            extract_status("[DONE]").unwrap().status,
            ExtendedStatus::Done
        );
        assert_eq!(
            extract_status("[Completed]").unwrap().status,
            ExtendedStatus::Done
        );
    }

    #[test]
    fn test_status_normalization_dev_complete() {
        assert_eq!(
            extract_status("[Dev Complete]").unwrap().status,
            ExtendedStatus::Review
        );
        assert_eq!(
            extract_status("[Development Complete]").unwrap().status,
            ExtendedStatus::Review
        );
    }

    #[test]
    fn test_status_normalization_in_progress() {
        let status = extract_status("[In Progress (8/9)]").unwrap();
        assert_eq!(status.status, ExtendedStatus::InProgress);
    }

    #[test]
    fn test_status_normalization_superseded() {
        let status = extract_status("[Superseded → See TEA-001]").unwrap();
        match status.status {
            ExtendedStatus::Superseded { see_also } => {
                assert_eq!(see_also, Some("TEA-001".to_string()));
            }
            _ => panic!("Expected Superseded status"),
        }
    }

    #[test]
    fn test_progress_extraction() {
        let status = extract_status("[In Progress (8/9 stories complete)]").unwrap();
        let progress = status.progress.unwrap();
        assert_eq!(progress.completed, 8);
        assert_eq!(progress.total, 9);
    }

    #[test]
    fn test_optional_flags() {
        let status = extract_status("[**Done** | **Optional/Experimental**]").unwrap();
        assert_eq!(status.status, ExtendedStatus::Done);
        assert!(status.optional);
        assert!(status.experimental);
    }

    #[test]
    fn test_notes_extraction() {
        let status = extract_status("[Done - All tasks complete]").unwrap();
        assert_eq!(status.notes, Some("All tasks complete".to_string()));
    }

    #[test]
    fn test_no_status_returns_none() {
        assert!(extract_status("Regular title without status").is_none());
        assert!(extract_status("# My Document").is_none());
    }

    #[test]
    fn test_draft_variants() {
        assert_eq!(
            extract_status("[Draft]").unwrap().status,
            ExtendedStatus::Draft
        );
        assert_eq!(
            extract_status("[TODO]").unwrap().status,
            ExtendedStatus::Draft
        );
        assert_eq!(
            extract_status("[Planned]").unwrap().status,
            ExtendedStatus::Draft
        );
    }

    #[test]
    fn test_cancelled_status() {
        assert_eq!(
            extract_status("[Cancelled]").unwrap().status,
            ExtendedStatus::Cancelled
        );
        assert_eq!(
            extract_status("[Canceled]").unwrap().status,
            ExtendedStatus::Cancelled
        );
    }

    #[test]
    fn test_deprecated_status() {
        assert_eq!(
            extract_status("[Deprecated]").unwrap().status,
            ExtendedStatus::Deprecated
        );
    }

    #[test]
    fn test_approved_status() {
        assert_eq!(
            extract_status("[Approved]").unwrap().status,
            ExtendedStatus::Approved
        );
    }

    #[test]
    fn test_extended_status_default() {
        assert_eq!(ExtendedStatus::default(), ExtendedStatus::Draft);
    }
}
