//! Retained native event facts and their interpretation, independent of a renderer.
use crate::{
    ItemEvent, ItemResult, OperationFailure, RemovedPart, RunOutcome, RunStart,
    vocabulary::{self, BatchTotals, Outcome, OutcomeFacts},
};
use std::path::{Path, PathBuf};

/// One settled item without borrowed message bytes or filesystem handles.
#[derive(Clone, Debug)]
pub struct SettledItem {
    /// Position in the discovered inventory, not emission order.
    pub index: usize,
    /// Native source operand, never reconstructed from display text.
    pub source: PathBuf,
    /// Intended native destination, independent of publication.
    pub destination: Option<PathBuf>,
    /// Independent outcome and receipt observations.
    pub facts: OutcomeFacts,
    /// Fully admitted removal metadata in source order.
    pub removed: Vec<RemovedPart>,
    /// Original size when metadata was established.
    pub source_bytes: Option<u64>,
    /// Candidate size when metadata was established.
    pub copy_bytes: Option<u64>,
    /// Actual operation failure, including its native cause.
    pub failure: Option<OperationFailure>,
    /// MIME section named by the failure, if any.
    pub section: Option<String>,
}
impl From<&ItemEvent<'_>> for SettledItem {
    fn from(item: &ItemEvent<'_>) -> Self {
        let metadata = item.result.metadata();
        let (failure, section) = match &item.result {
            ItemResult::Failed(f) => (Some(f.error().clone()), f.section().map(str::to_owned)),
            _ => (None, None),
        };
        Self {
            index: item.index,
            source: item.source.native.to_owned(),
            destination: item.destination.map(|p| p.native.to_owned()),
            facts: OutcomeFacts::from(&item.result),
            removed: metadata.map_or_else(Vec::new, |m| m.removed.to_vec()),
            source_bytes: metadata.map(|m| m.source_bytes),
            copy_bytes: metadata.map(|m| m.copy_bytes),
            failure,
            section,
        }
    }
}
impl SettledItem {
    /// Canonical interpretation of this item and its removal metadata.
    pub fn outcome(&self) -> Outcome {
        Outcome::from(&self.facts, self.removed.len())
    }
    /// Candidate reduction, not space freed or proof that a copy exists.
    pub fn copy_size_reduction(&self) -> Option<u64> {
        self.source_bytes?.checked_sub(self.copy_bytes?)
    }
    /// Earlier independent receipts permit navigation, without current requalification.
    pub fn reveal_path(&self) -> Option<&Path> {
        let f = &self.facts;
        let qualified = crate::qualification::publication(
            f.publication,
            f.candidate_identity_known,
            f.address,
            f.file_sync,
            f.namespace_sync,
            &f.cleanup,
        ) || f.existing.as_ref().is_some_and(|e| {
            crate::qualification::existing(
                e.identity_known,
                e.bytes_equal,
                e.stable,
                e.address,
                e.file_sync,
                e.namespace_sync,
            )
        });
        self.destination
            .as_deref()
            .filter(|p| qualified && p.is_absolute())
    }
}
impl RunStart {
    /// Human inventory or refusal label, without claiming a partial prefix is complete.
    pub fn inventory_label(&self) -> String {
        self.run_error
            .as_ref()
            .and_then(OperationFailure::refusal_headline)
            .unwrap_or_else(|| vocabulary::email_count(self.total))
    }

    /// Unreported discovered positions after an incomplete settled run.
    /// A summary owns remaining-work counts whenever one exists.
    pub fn unreported_item_count(
        &self,
        reported: usize,
        summary: Option<&RunOutcome>,
        settled: bool,
    ) -> Option<usize> {
        if !settled || summary.is_some() {
            return None;
        }
        self.total.checked_sub(reported)
    }
}
impl RunOutcome {
    /// Human totals from terminal observations and never-started positions, counted once.
    pub fn settled_totals(&self, items: &[SettledItem]) -> BatchTotals {
        BatchTotals::from(
            items.iter().map(|i| {
                (
                    Some(i.outcome()),
                    i.removed.len(),
                    i.source_bytes.zip(i.copy_bytes),
                )
            }),
            self.counts().not_processed(),
        )
    }
    /// Whether all discovered work is accounted for without delivery uncertainty.
    pub fn results_cover_inventory(&self) -> bool {
        self.delivery_failure().is_none()
            && vocabulary::results_cover_inventory(
                self.collection_complete(),
                self.interrupted(),
                self.counts().not_processed(),
                self.run_error().map(OperationFailure::code),
            )
    }
}
