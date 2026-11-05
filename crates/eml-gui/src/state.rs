//! Pure draft, queue and navigation state. No toolkit or I/O dependencies.
use eml_app::{RequestSnapshot, RunId, RunQueue};
use eml_core::RunMode;
use std::path::PathBuf;

pub(super) enum PendingConfirmation {
    ForceStop(RunId),
    Quit,
    ReplaceDraft(RequestSnapshot),
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum View {
    #[default]
    Draft,
    Run(RunId),
    Waiting(RunId),
}
pub(super) struct Session {
    pub view: View,
    pub confirmation: Option<PendingConfirmation>,
    pub has_started: bool,
    pub selection: Vec<PathBuf>,
    pub queue: RunQueue,
    pub mode: RunMode,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            view: View::Draft,
            confirmation: None,
            has_started: false,
            selection: Vec::new(),
            queue: RunQueue::default(),
            mode: RunMode::Create,
        }
    }
}
#[derive(Default)]
pub(super) struct RunPresentation {
    pub unseen: bool,
    pub filter: Filter,
    pub sort: Sort,
    pub selected: Option<usize>,
    pub initialized: bool,
    pub tally: Tally,
    pub total: Option<u32>,
}
impl RunPresentation {
    pub(super) fn settle(&mut self, visible: bool) {
        self.unseen = !visible;
    }
    pub(super) fn visit(&mut self) {
        self.unseen = false;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Category {
    Created,
    Checked,
    UpToDate,
    Attention,
    NotProcessed,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Filter {
    #[default]
    All,
    Created,
    Checked,
    UpToDate,
    Attention,
    NotProcessed,
}
impl Filter {
    pub(super) fn from_index(index: i32) -> Option<Self> {
        Some(match index {
            0 => Self::All,
            1 => Self::Created,
            2 => Self::Checked,
            3 => Self::UpToDate,
            4 => Self::Attention,
            5 => Self::NotProcessed,
            _ => return None,
        })
    }
    pub(super) fn index(self) -> i32 {
        match self {
            Self::All => 0,
            Self::Created => 1,
            Self::Checked => 2,
            Self::UpToDate => 3,
            Self::Attention => 4,
            Self::NotProcessed => 5,
        }
    }
    pub(super) fn includes(self, category: Category) -> bool {
        match self {
            Self::All => true,
            Self::Created => category == Category::Created,
            Self::Checked => category == Category::Checked,
            Self::UpToDate => category == Category::UpToDate,
            Self::Attention => category == Category::Attention,
            Self::NotProcessed => category == Category::NotProcessed,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Column {
    Message,
    #[default]
    Folder,
    Outcome,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Sort {
    pub column: Column,
    pub descending: bool,
}
impl Default for Sort {
    fn default() -> Self {
        Self {
            column: Column::Folder,
            descending: false,
        }
    }
}
impl Sort {
    pub(super) fn select(&mut self, column: Column) {
        if self.column == column {
            self.descending = !self.descending;
        } else {
            self.column = column;
            self.descending = false;
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Tally {
    pub all: u32,
    pub created: u32,
    pub checked: u32,
    pub up_to_date: u32,
    pub attention: u32,
    pub not_processed: u32,
    pub processed: u32,
}
impl Tally {
    pub(super) fn observe(&mut self, category: Category) {
        match category {
            Category::Created => self.created += 1,
            Category::Checked => self.checked += 1,
            Category::UpToDate => self.up_to_date += 1,
            Category::Attention => self.attention += 1,
            Category::NotProcessed => self.not_processed += 1,
        }
        if category != Category::NotProcessed {
            self.processed += 1;
        }
    }
}
/// GUI grouping preserves core qualification. An explicit interruption is not an error verdict.
pub(super) fn category(
    meaning: Option<(
        eml_core::vocabulary::MeaningKind,
        eml_core::vocabulary::Severity,
    )>,
    interrupted: bool,
) -> Category {
    use eml_core::vocabulary::{MeaningKind, Severity};
    if interrupted {
        return Category::NotProcessed;
    }
    match meaning {
        Some((_, Severity::Attention)) | None => Category::Attention,
        Some((MeaningKind::Created, _)) => Category::Created,
        Some((MeaningKind::Prepared | MeaningKind::Comparison, _)) => Category::Checked,
        Some((MeaningKind::ExistingVerified, _)) => Category::UpToDate,
        Some((
            MeaningKind::PublishedIncomplete
            | MeaningKind::ExistingMatched
            | MeaningKind::PublicationUncertain
            | MeaningKind::NotPublished
            | MeaningKind::Unestablished,
            _,
        )) => Category::Attention,
    }
}

/// ASCII digit runs compare without integer conversion; byte order breaks equal numeric keys.
pub(super) fn natural_cmp(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> std::cmp::Ordering {
    use std::{cmp::Ordering, os::unix::ffi::OsStrExt};
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (mut ae, mut be) = (i, j);
            while ae < a.len() && a[ae].is_ascii_digit() {
                ae += 1;
            }
            while be < b.len() && b[be].is_ascii_digit() {
                be += 1;
            }
            while i < ae && a[i] == b'0' {
                i += 1;
            }
            while j < be && b[j] == b'0' {
                j += 1;
            }
            let order = (ae - i)
                .cmp(&(be - j))
                .then_with(|| a[i..ae].cmp(&b[j..be]));
            if order != Ordering::Equal {
                return order;
            }
            (i, j) = (ae, be);
        } else {
            let order = a[i].cmp(&b[j]);
            if order != Ordering::Equal {
                return order;
            }
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j)).then_with(|| a.cmp(b))
}

#[derive(Default)]
pub(super) struct ProgressEstimate {
    started: u64,
    sampled: u64,
    completed: u32,
    rate: Option<f64>,
}
impl ProgressEstimate {
    pub(super) fn start(now: u64) -> Self {
        Self {
            started: now,
            sampled: now,
            ..Default::default()
        }
    }
    pub(super) fn observe(&mut self, now: u64, completed: u32) {
        if completed > self.completed && now > self.sampled {
            let rate = f64::from(completed - self.completed) * 1000.0 / (now - self.sampled) as f64;
            self.rate = Some(
                self.rate
                    .map_or(rate, |previous| previous * 0.8 + rate * 0.2),
            );
            self.sampled = now;
            self.completed = completed;
        }
    }
    pub(super) fn remaining(&self, now: u64, total: Option<u32>) -> Option<u64> {
        let total = total?;
        if self.completed < 10 || now.saturating_sub(self.started) < 3000 {
            return None;
        }
        self.rate
            .map(|rate| (f64::from(total.saturating_sub(self.completed)) / rate).ceil() as u64)
    }
}

#[cfg(test)]
mod contracts {
    use super::*;
    #[test]
    fn s6_natural_comparison_handles_long_digit_runs_zeroes_and_non_utf8() {
        use std::{
            cmp::Ordering,
            ffi::{OsStr, OsString},
            os::unix::ffi::OsStringExt,
        };
        for (a, b) in [
            ("n1", "n2"),
            ("n2", "n10"),
            ("n0", "n000"),
            ("n02z", "n10a"),
            ("x999999999999999999999999", "x1000000000000000000000000"),
        ] {
            assert_eq!(natural_cmp(OsStr::new(a), OsStr::new(b)), Ordering::Less);
            assert_eq!(natural_cmp(OsStr::new(b), OsStr::new(a)), Ordering::Greater);
        }
        let a = OsString::from_vec(b"a\xff2".to_vec());
        let b = OsString::from_vec(b"a\xff10".to_vec());
        assert_eq!(natural_cmp(&a, &b), Ordering::Less);
    }
    #[test]
    fn s4_interruption_is_not_attention_and_unqualified_copies_still_are() {
        use eml_core::vocabulary::{MeaningKind, Severity};
        assert_eq!(
            category(Some((MeaningKind::NotPublished, Severity::Attention)), true),
            Category::NotProcessed
        );
        assert_eq!(
            category(
                Some((MeaningKind::ExistingMatched, Severity::Attention)),
                false
            ),
            Category::Attention
        );
        let mut tally = Tally::default();
        tally.observe(Category::NotProcessed);
        assert_eq!(
            (tally.processed, tally.attention, tally.not_processed),
            (0, 0, 1)
        );
    }
    #[test]
    fn estimate_requires_ten_results_three_seconds_and_a_known_total() {
        let mut estimate = ProgressEstimate::start(0);
        estimate.observe(1000, 10);
        assert_eq!(estimate.remaining(2999, Some(100)), None);
        assert_eq!(estimate.remaining(3000, None), None);
        assert_eq!(estimate.remaining(3000, Some(100)), Some(9));
        estimate.observe(4000, 11);
        assert!(estimate.remaining(4000, Some(100)).unwrap() > 9);
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) enum Stop {
    #[default]
    Running,
    Requested {
        at: u64,
    },
    Forced,
}
impl Stop {
    pub(super) fn can_force(self, now: u64) -> bool {
        matches!(self,Self::Requested{at} if now.saturating_sub(at)>=5000)
    }
}
impl RunPresentation {
    pub(super) fn reconcile(&mut self, categories: &[Category], settled: bool) {
        if settled && !self.initialized {
            self.filter = if categories.contains(&Category::Attention) {
                Filter::Attention
            } else {
                Filter::All
            };
            self.selected = None;
            self.initialized = true;
        }
        if self.selected.is_none_or(|index| {
            categories
                .get(index)
                .is_none_or(|category| !self.filter.includes(*category))
        }) {
            self.selected = categories
                .iter()
                .position(|category| self.filter.includes(*category));
        }
    }
}

/// Transient feedback, separate from immutable processing observations.
#[derive(Default)]
pub(super) enum Feedback {
    #[default]
    None,
    Message {
        label: String,
        text: String,
        details: String,
        tone: FeedbackTone,
    },
    SavedReport {
        run: eml_app::RunId,
        path: std::path::PathBuf,
        at: u64,
    },
}
#[derive(Clone, Copy)]
pub(super) enum FeedbackTone {
    Info,
    Success,
    Attention,
}
