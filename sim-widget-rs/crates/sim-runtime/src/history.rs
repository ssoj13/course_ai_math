use sim_model::*;
#[derive(Clone, Debug)]
pub(crate) struct HistoryEntry {
    pub before: ValidatedDocument,
    pub after: ValidatedDocument,
}
impl HistoryEntry {
    pub fn bytes(&self, limit: usize) -> Result<usize> {
        let a = self
            .before
            .allocated_bytes()?
            .max(bounded_size(self.before.document(), limit)?);
        let b = self.after.allocated_bytes()?.max(bounded_size(
            self.after.document(),
            limit.saturating_sub(a),
        )?);
        a.checked_add(b).filter(|n| *n <= limit).ok_or_else(|| {
            Diagnostic::new(DiagnosticCode::Limit, "history", "history byte overflow")
        })
    }
}
