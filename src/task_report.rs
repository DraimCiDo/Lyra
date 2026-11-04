//! Bound durable diagnostics independently of the model's context window.
use crate::agent::{Event, Report, VerificationEvidence};
use crate::executor::VerificationResult;
use std::io;
pub const PROPOSAL_LIMIT: usize = 1024 * 1024;
fn head(text: &str, limit: usize) -> &str {
    let mut end = limit.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
fn clipped(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.into();
    }
    let front = limit / 2;
    let mut tail = text.len() - limit / 2;
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    format!(
        "{}\n[...truncated...]\n{}",
        head(text, front),
        &text[tail..]
    )
}
pub fn diagnostic(text: &str) -> String {
    clipped(text, 1024)
}
impl Report {
    pub(crate) fn record_event(&mut self, event: Event) {
        crate::task_report::event(self, event);
    }
}
pub fn event(report: &mut Report, mut event: Event) {
    event.outcome = clipped(&event.outcome, 512);
    event.argument = event.argument.map(|s| clipped(&s, 256));
    report.events.push(event);
    if report.events.len() > 32 {
        let count = report.events.len() - 32;
        report.events.drain(..count);
        report.events_omitted += count as u32;
    }
}
pub fn summarize(report: &mut Report, role: &str, summary: &str) -> io::Result<()> {
    if summary.trim().is_empty() || summary.len() > 8192 {
        return Err(io::Error::other("Summary requires 1..8192 UTF-8 bytes"));
    }
    let mut proposed = report.summaries.clone();
    proposed.push((role.into(), summary.into()));
    if serde_json::to_vec(&proposed)
        .map_err(io::Error::other)?
        .len()
        > 64 * 1024
    {
        return Err(io::Error::other("Summary history exceeds 64 KiB"));
    }
    report.summaries = proposed;
    Ok(())
}
pub fn proposal_fits(
    report: &Report,
    edit: &crate::workspace::Edit,
    index: Option<usize>,
) -> io::Result<bool> {
    let mut proposed = report.proposals.clone();
    if let Some(index) = index {
        proposed[index] = edit.clone();
    } else {
        proposed.push(edit.clone());
    }
    Ok(serde_json::to_vec(&proposed)
        .map_err(io::Error::other)?
        .len()
        <= PROPOSAL_LIMIT)
}
pub fn evidence(
    report: &mut Report,
    proposal_sha256: String,
    check_sha256: String,
    mut result: VerificationResult,
) -> io::Result<serde_json::Value> {
    report.verification_scope = std::mem::take(&mut result.source_scope);
    let digest = if result.stdout.len() > 16 * 1024 || result.stderr.len() > 16 * 1024 {
        let bytes =
            serde_json::to_vec(&(&result.stdout, &result.stderr)).map_err(io::Error::other)?;
        result.stdout = clipped(&result.stdout, 16 * 1024);
        result.stderr = clipped(&result.stderr, 16 * 1024);
        result.output_truncated = true;
        Some(crate::retrieval::sha256(&bytes))
    } else {
        None
    };
    let data = serde_json::to_value(&result).map_err(io::Error::other)?;
    report.verifications.push(VerificationEvidence {
        proposal_sha256,
        check_sha256,
        result,
        captured_output_sha256: digest,
    });
    while report.verifications.len() > 1
        && (report.verifications.len() > 8
            || serde_json::to_vec(&report.verifications)
                .map_err(io::Error::other)?
                .len()
                > 256 * 1024)
    {
        report.verifications.remove(0);
        report.verifications_omitted += 1;
    }
    Ok(data)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_large_checks_and_diagnostics_leave_a_bounded_report_and_current_proof() {
        let mut report:Report=serde_json::from_value(serde_json::json!({"schema_version":4,"workspace":"root","status":"completed","outcome":"changes_verified", "proposals":[],"summaries":[],"events":[],"calls":200,"subagents":false,"truncated_index":false,"model_review_is_verified_test":false})).unwrap();
        for call in 0..200 {
            event(
                &mut report,
                Event {
                    call,
                    role: "coder".into(),
                    tool: "verify".into(),
                    outcome: "x".repeat(64000),
                    argument: None,
                },
            );
            evidence(
                &mut report,
                format!("proposal-{call}"),
                "check".into(),
                VerificationResult {
                    exit_code: Some(0),
                    timed_out: false,
                    cancelled: false,
                    stdout: "\u{1}".repeat(65536),
                    stderr: "y".repeat(65536),
                    output_truncated: false,
                    elapsed_ms: 1,
                    cleanup_confirmed: true,
                    source_sha256: None,
                    source_scope: Vec::new(),
                },
            )
            .unwrap();
        }
        assert!(report.events_omitted > 0 && report.verifications_omitted > 0);
        assert_eq!(
            report.verifications.last().unwrap().proposal_sha256,
            "proposal-199"
        );
        assert!(report.verifications.last().unwrap().result.passed());
        assert!(
            report
                .verifications
                .last()
                .unwrap()
                .captured_output_sha256
                .is_some()
        );
        assert!(serde_json::to_vec(&report).unwrap().len() < 512 * 1024);
    }
}
