/// Diagnostics — convert analysis errors into LSP diagnostics.

use crate::analysis::{self, AnalysisError, AnalysisResult};
use tower_lsp::lsp_types::*;

/// Run analysis on source text and return LSP diagnostics.
pub fn compute_diagnostics(source: &str) -> Vec<Diagnostic> {
    let result = analysis::analyze(source);
    errors_to_diagnostics(&result)
}

/// Convert analysis errors to LSP Diagnostic values.
fn errors_to_diagnostics(result: &AnalysisResult) -> Vec<Diagnostic> {
    result
        .errors
        .iter()
        .map(error_to_diagnostic)
        .chain(result.lints.iter().map(lint_to_diagnostic))
        .collect()
}

/// Render a linter or resolver finding. These carry a line but no column, so
/// the whole line is highlighted.
fn lint_to_diagnostic(lint: &que_lang::linter::LintDiagnostic) -> Diagnostic {
    let line = lint.line.unwrap_or(1).saturating_sub(1) as u32;
    Diagnostic {
        range: Range::new(Position::new(line, 0), Position::new(line, u32::MAX)),
        severity: Some(match lint.severity {
            que_lang::linter::Severity::Error => DiagnosticSeverity::ERROR,
            que_lang::linter::Severity::Warning => DiagnosticSeverity::WARNING,
        }),
        code: Some(NumberOrString::String(lint.rule.to_string())),
        code_description: None,
        source: Some("que".to_string()),
        message: lint.message.clone(),
        related_information: None,
        tags: None,
        data: None,
    }
}

fn error_to_diagnostic(error: &AnalysisError) -> Diagnostic {
    let range = if let Some(span) = &error.span {
        analysis::span_to_range(span)
    } else {
        Range::new(Position::new(0, 0), Position::new(0, 0))
    };

    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::ERROR),
        code: None,
        code_description: None,
        source: Some("que".to_string()),
        message: error.message.clone(),
        related_information: None,
        tags: None,
        data: None,
    }
}
