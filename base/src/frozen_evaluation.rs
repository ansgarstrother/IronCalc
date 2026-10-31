//! Explicit, per-model inputs for volatile formula evaluation.
//!
//! This module controls time and random draws. It does not authenticate a workbook,
//! its viewer, or the supplied inputs, and does not define canonical XLSX bytes.

use chrono::DateTime;
use sha2::{Digest, Sha256};

use crate::{
    calc_result::CalcResult,
    expressions::{token::Error, types::CellReferenceIndex},
    model::{CellState, Model},
    types::Workbook,
};

/// Version of the fixed-clock and per-cell random-stream contract.
///
/// A random draw hashes the UTF-8 domain followed by a NUL byte, the 32-byte seed,
/// sheet (u32), row and column (i32), and zero-based draw index (u64), all integers
/// in big-endian order. The upper 53 bits of the first eight digest bytes are
/// divided by 2^53. Streams reset on each complete evaluation pass.
///
/// This profile does not promise cross-platform equivalence for every numerical
/// function or deterministic workbook serialization.
pub const FROZEN_EVALUATION_PROFILE: &str = "ironcalc.frozen-evaluation.random-cell.v1";

/// Immutable time and random seed for one model.
///
/// The host must retain and bind these inputs to its own source and evaluation
/// identity. The seed provides reproducibility, not a cryptographic RNG service.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrozenEvaluationContext {
    milliseconds_since_epoch: i64,
    random_seed: [u8; 32],
}

impl FrozenEvaluationContext {
    /// Constructs explicit inputs, refusing timestamps outside Chrono's range.
    pub fn new(
        milliseconds_since_epoch: i64,
        random_seed: [u8; 32],
    ) -> Result<Self, FrozenEvaluationError> {
        if DateTime::from_timestamp_millis(milliseconds_since_epoch).is_none() {
            return Err(FrozenEvaluationError::InvalidTimestamp);
        }
        Ok(Self {
            milliseconds_since_epoch,
            random_seed,
        })
    }

    /// Returns the fixed UTC timestamp, in milliseconds since the Unix epoch.
    pub fn milliseconds_since_epoch(&self) -> i64 {
        self.milliseconds_since_epoch
    }

    /// Returns the exact seed retained for this model.
    pub fn random_seed(&self) -> &[u8; 32] {
        &self.random_seed
    }
}

/// An operational frozen-evaluation failure, distinct from a formula error.
///
/// A formula such as IFERROR may handle formula errors. It must not turn an
/// operational failure into a successful frozen-evaluation result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrozenEvaluationError {
    /// The model was constructed with the normal ambient environment.
    NotFrozen,
    /// The supplied timestamp cannot be represented by Chrono.
    InvalidTimestamp,
    /// A random draw was requested outside an actively evaluated formula cell.
    MissingCellEvaluation,
    /// A cell exhausted its u64 random-draw counter.
    RandomDrawOverflow,
}

impl std::fmt::Display for FrozenEvaluationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotFrozen => "model does not have frozen evaluation inputs",
            Self::InvalidTimestamp => "frozen timestamp is outside the supported range",
            Self::MissingCellEvaluation => "random draw has no active formula cell",
            Self::RandomDrawOverflow => "formula cell exhausted its random-draw counter",
        })
    }
}

impl std::error::Error for FrozenEvaluationError {}

#[derive(Clone, Copy)]
pub(crate) enum EvaluationEnvironment {
    Ambient,
    Frozen(FrozenEvaluationContext),
}

impl EvaluationEnvironment {
    pub(crate) fn milliseconds_since_epoch(self) -> i64 {
        match self {
            Self::Ambient => crate::model::get_milliseconds_since_epoch(),
            Self::Frozen(context) => context.milliseconds_since_epoch,
        }
    }
}

fn random_cell(context: FrozenEvaluationContext, cell: CellReferenceIndex, draw: u64) -> f64 {
    let mut hash = Sha256::new();
    hash.update(FROZEN_EVALUATION_PROFILE.as_bytes());
    hash.update([0]);
    hash.update(context.random_seed);
    hash.update(cell.sheet.to_be_bytes());
    hash.update(cell.row.to_be_bytes());
    hash.update(cell.column.to_be_bytes());
    hash.update(draw.to_be_bytes());
    let digest = hash.finalize();
    let mut first = [0; 8];
    first.copy_from_slice(&digest[..8]);
    (u64::from_be_bytes(first) >> 11) as f64 / 9_007_199_254_740_992.0
}

#[cfg(not(target_arch = "wasm32"))]
fn ambient_random() -> f64 {
    rand::random()
}

#[cfg(target_arch = "wasm32")]
fn ambient_random() -> f64 {
    js_sys::Math::random()
}

impl<'a> Model<'a> {
    /// Creates a new model whose metadata, NOW, TODAY, RAND and RANDBETWEEN
    /// use the explicit immutable inputs. No ambient clock is read.
    pub fn new_empty_with_frozen_context(
        name: &'a str,
        locale_id: &'a str,
        timezone: &'a str,
        language_id: &'a str,
        context: FrozenEvaluationContext,
    ) -> Result<Model<'a>, String> {
        Self::new_empty_with_environment(
            name,
            locale_id,
            timezone,
            language_id,
            EvaluationEnvironment::Frozen(context),
        )
    }

    /// Loads an existing workbook with explicit volatile-evaluation inputs.
    ///
    /// Existing metadata is preserved. Reevaluate with evaluate_frozen before
    /// using formula results; cached workbook values are not evaluation proof.
    pub fn from_workbook_with_frozen_context(
        workbook: Workbook,
        language_id: &str,
        context: FrozenEvaluationContext,
    ) -> Result<Model<'_>, String> {
        Self::from_workbook_with_environment(
            workbook,
            language_id,
            EvaluationEnvironment::Frozen(context),
        )
    }

    /// Decodes workbook bytes with explicit volatile-evaluation inputs.
    ///
    /// The context is runtime state and is not serialized in workbook bytes.
    pub fn from_bytes_with_frozen_context(
        bytes: &[u8],
        language_id: &'a str,
        context: FrozenEvaluationContext,
    ) -> Result<Model<'a>, String> {
        let workbook: Workbook =
            bitcode::decode(bytes).map_err(|e| format!("Error parsing workbook: {e}"))?;
        Self::from_workbook_with_environment(
            workbook,
            language_id,
            EvaluationEnvironment::Frozen(context),
        )
    }

    /// Returns explicit inputs, or None for an ordinary interactive model.
    pub fn frozen_evaluation_context(&self) -> Option<FrozenEvaluationContext> {
        match self.evaluation_environment {
            EvaluationEnvironment::Ambient => None,
            EvaluationEnvironment::Frozen(context) => Some(context),
        }
    }

    /// Reads this model's fixed timestamp, or the normal ambient clock.
    ///
    /// Frozen hosts can use this for export metadata. It does not make XLSX
    /// serialization canonical or authenticate the timestamp.
    pub fn evaluation_timestamp_millis(&self) -> i64 {
        self.evaluation_environment.milliseconds_since_epoch()
    }

    /// Runs the actual evaluator with immutable volatile inputs.
    ///
    /// Refuses ambient models before evaluation. Repeated calls reset every
    /// cell's random stream. Formula errors remain ordinary cell results;
    /// operational failures are returned even when a formula handles #ERROR.
    pub fn evaluate_frozen(&mut self) -> Result<(), FrozenEvaluationError> {
        if self.frozen_evaluation_context().is_none() {
            return Err(FrozenEvaluationError::NotFrozen);
        }
        self.evaluate();
        self.frozen_evaluation_result()
    }

    fn frozen_evaluation_result(&self) -> Result<(), FrozenEvaluationError> {
        if self.frozen_evaluation_context().is_none() {
            return Err(FrozenEvaluationError::NotFrozen);
        }
        match self.frozen_evaluation_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub(crate) fn next_random(&mut self, cell: CellReferenceIndex) -> Result<f64, CalcResult> {
        let context = match self.evaluation_environment {
            EvaluationEnvironment::Ambient => return Ok(ambient_random()),
            EvaluationEnvironment::Frozen(context) => context,
        };
        let key = (cell.sheet, cell.row, cell.column);
        let failure = match self.cells.get_mut(&key) {
            Some(CellState::Evaluating { random_draws }) => match random_draws.checked_add(1) {
                Some(next) => {
                    let draw = *random_draws;
                    *random_draws = next;
                    return Ok(random_cell(context, cell, draw));
                }
                None => FrozenEvaluationError::RandomDrawOverflow,
            },
            _ => FrozenEvaluationError::MissingCellEvaluation,
        };
        self.frozen_evaluation_error.get_or_insert(failure);
        Err(CalcResult::new_error(
            Error::ERROR,
            cell,
            failure.to_string(),
        ))
    }
}

#[cfg(test)]
mod tests;
