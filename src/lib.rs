//! Text to speech for Native Shell apps. Scaffold only: see docs/brief.md.

/// Reports that this crate has no implementation yet.
pub fn status() -> &'static str {
    "scaffold"
}

#[cfg(test)]
mod tests {
    #[test]
    fn scaffold_reports_status() {
        assert_eq!(super::status(), "scaffold");
    }
}
