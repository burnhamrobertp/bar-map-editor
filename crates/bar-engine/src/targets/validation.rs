//! Validation errors for export targets.

/// A validation error found during pre-export checks.
#[derive(Debug, Clone)]
pub struct ValidationError {
    /// Severity of the issue.
    pub severity: Severity,
    /// Which layer or component has the issue.
    pub component: String,
    /// Human-readable description of the problem.
    pub message: String,
}

/// Severity level for validation issues.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Severity {
    /// Export will fail — must be fixed.
    Error,
    /// Export may produce suboptimal results.
    Warning,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let level = match self.severity {
            Severity::Error => "ERROR",
            Severity::Warning => "WARN",
        };
        write!(f, "[{}] {}: {}", level, self.component, self.message)
    }
}
