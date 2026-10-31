#![allow(clippy::unwrap_used)]

use crate::{
    cell::CellValue, expressions::types::CellReferenceIndex, model::CellState,
    FrozenEvaluationContext, FrozenEvaluationError, Model,
};

const TIME: i64 = 1_679_319_865_208;
const FIRST_A1_BITS: u64 = 0x3fe4_c575_8103_4700;
const SECOND_A1_BITS: u64 = 0x3fe2_f420_c7b8_1846;

fn context(seed: u8) -> FrozenEvaluationContext {
    FrozenEvaluationContext::new(TIME, [seed; 32]).unwrap()
}

fn model() -> Model<'static> {
    Model::new_empty_with_frozen_context("frozen", "en", "UTC", "en", context(7)).unwrap()
}

fn number(model: &Model<'_>, reference: &str) -> f64 {
    let cell = model._parse_reference(reference);
    match model
        .get_cell_value_by_index(cell.sheet, cell.row, cell.column)
        .unwrap()
    {
        CellValue::Number(value) => value,
        other => panic!("expected numeric {reference}, got {other:?}"),
    }
}

#[test]
fn fixed_time_controls_formulas_and_new_metadata_without_ambient_clock() {
    // The ordinary test clock deliberately differs from the supplied timestamp.
    let mut utc = model();
    assert_eq!(utc.evaluation_timestamp_millis(), TIME);
    assert_eq!(utc.workbook.metadata.created, "2023-03-20T13:44:25Z");
    assert_eq!(utc.workbook.metadata.last_modified, "2023-03-20T13:44:25Z");
    utc._set("A1", "=NOW()");
    utc._set("A2", "=TODAY()");
    utc._set("A3", "=NOW(\"America/New_York\")");
    utc.evaluate_frozen().unwrap();
    assert_eq!(number(&utc, "A1"), 45_005.0 + 49_465.0 / 86_400.0);
    assert_eq!(number(&utc, "A2"), 45_005.0);
    assert_eq!(number(&utc, "A3"), 45_005.0 + 35_065.0 / 86_400.0);

    let mut new_york =
        Model::new_empty_with_frozen_context("frozen", "en", "America/New_York", "en", context(7))
            .unwrap();
    new_york._set("A1", "=NOW()");
    new_york._set("A2", "=NOW(\"UTC\")");
    new_york.evaluate_frozen().unwrap();
    assert_eq!(number(&new_york, "A1"), number(&utc, "A3"));
    assert_eq!(number(&new_york, "A2"), number(&utc, "A1"));
}

#[test]
fn context_is_immutable_and_loading_requires_explicit_context_again() {
    let fixed = context(7);
    assert_eq!(fixed.milliseconds_since_epoch(), TIME);
    assert_eq!(fixed.random_seed(), &[7; 32]);
    assert_eq!(
        FrozenEvaluationContext::new(i64::MAX, [0; 32]),
        Err(FrozenEvaluationError::InvalidTimestamp),
    );
    let mut original = model();
    original._set("A1", "=RAND()");
    original.evaluate_frozen().unwrap();
    let first = number(&original, "A1");
    let bytes = original.to_bytes();

    let mut ambient = Model::from_bytes(&bytes, "en").unwrap();
    assert_eq!(ambient.frozen_evaluation_context(), None);
    assert_eq!(
        ambient.evaluate_frozen(),
        Err(FrozenEvaluationError::NotFrozen)
    );

    let mut restored = Model::from_bytes_with_frozen_context(&bytes, "en", fixed).unwrap();
    restored.evaluate_frozen().unwrap();
    assert_eq!(number(&restored, "A1"), first);
    assert_eq!(
        restored.workbook.metadata.created,
        original.workbook.metadata.created
    );

    let mut other =
        Model::from_workbook_with_frozen_context(original.workbook, "en", context(8)).unwrap();
    // Cached values are not accepted as results for a different context.
    other.evaluate_frozen().unwrap();
    assert_ne!(number(&other, "A1"), first);
    assert_eq!(other.workbook.metadata.created, "2023-03-20T13:44:25Z");
}

#[test]
fn cell_stream_matches_versioned_golden_and_resets_each_pass() {
    // Independently computed from the documented big-endian SHA-256 framing.
    let mut actual = model();
    actual._set("A1", "=RAND()");
    actual._set("B1", "=A1+A1");
    actual._set("A2", "=RAND()");
    actual.evaluate_frozen().unwrap();
    let first = number(&actual, "A1");
    let second_cell = number(&actual, "A2");
    assert_eq!(first.to_bits(), FIRST_A1_BITS);
    assert!((0.0..1.0).contains(&second_cell));
    assert_ne!(first, second_cell);
    assert_eq!(number(&actual, "B1"), first + first);
    actual.evaluate_frozen().unwrap();
    assert_eq!(number(&actual, "A1"), first);
    assert_eq!(number(&actual, "A2"), second_cell);
    assert_eq!(number(&actual, "B1"), first + first);
}

#[test]
fn random_draws_do_not_depend_on_cell_insertion_or_dependency_traversal_order() {
    let entries = [
        ("A1", "=RAND()+RAND()"),
        ("B1", "=A1+A2"),
        ("A2", "=RAND()"),
        ("C1", "=RAND()"),
        ("D1", "=B1+C1"),
    ];
    let mut forward = model();
    let mut reverse = model();
    for (cell, formula) in entries {
        forward._set(cell, formula);
    }
    for (cell, formula) in entries.into_iter().rev() {
        reverse._set(cell, formula);
    }
    forward.evaluate_frozen().unwrap();
    reverse.evaluate_frozen().unwrap();
    assert_eq!(
        number(&forward, "A1"),
        f64::from_bits(FIRST_A1_BITS) + f64::from_bits(SECOND_A1_BITS),
    );
    for (cell, _) in entries {
        assert_eq!(
            number(&forward, cell).to_bits(),
            number(&reverse, cell).to_bits(),
            "{cell}"
        );
    }
}

#[test]
fn lazy_branches_and_invalid_arguments_consume_only_executed_draws() {
    for formula in [
        "=IF(FALSE,RAND(),RAND())",
        "=IF(TRUE,RAND(),RAND())",
        "=IFERROR(1/0,RAND())",
        "=IFERROR(RAND(1),RAND())",
        "=IFERROR(RANDBETWEEN(\"bad\",10),RAND())",
        "=IFERROR(RANDBETWEEN(20,10),RAND())",
    ] {
        let mut actual = model();
        actual._set("A1", formula);
        actual.evaluate_frozen().unwrap();
        assert_eq!(number(&actual, "A1").to_bits(), FIRST_A1_BITS, "{formula}");
    }
    let mut actual = model();
    actual._set("A1", "=RANDBETWEEN(RAND(),10)");
    actual.evaluate_frozen().unwrap();
    // The lower-bound RAND executes first; RANDBETWEEN consumes the next draw.
    assert_eq!(
        number(&actual, "A1"),
        (f64::from_bits(SECOND_A1_BITS) * 11.0).floor()
    );
}

#[test]
fn ordinary_formula_errors_keep_existing_semantics() {
    let mut actual = model();
    actual._set("A1", "=NOW(\"not-a-timezone\")");
    actual._set("A2", "=TODAY(1)");
    actual._set("A3", "=RAND(1)");
    actual._set("A4", "=RANDBETWEEN(20,10)");
    actual._set("A5", "=IFERROR(RANDBETWEEN(\"bad\",10),42)");
    actual.evaluate_frozen().unwrap();
    assert_eq!(actual._get_text("A1"), "#VALUE!");
    assert_eq!(actual._get_text("A2"), "#ERROR!");
    assert_eq!(actual._get_text("A3"), "#ERROR!");
    assert_eq!(actual._get_text("A4"), "#NUM!");
    assert_eq!(number(&actual, "A5"), 42.0);
}

#[test]
fn iferror_cannot_erase_an_operational_random_failure() {
    let cell = CellReferenceIndex {
        sheet: 0,
        row: 1,
        column: 1,
    };
    for overflow in [false, true] {
        let mut actual = model();
        actual._set("A1", "=IFERROR(RAND(),123)");
        let formula = actual._get_cell("A1").get_formula().unwrap() as usize;
        let node = actual.parsed_formulas[0][formula].clone();
        if overflow {
            // Fault injection at the actual per-cell evaluator state boundary.
            actual.cells.insert(
                (0, 1, 1),
                CellState::Evaluating {
                    random_draws: u64::MAX,
                },
            );
        }
        let result = actual.evaluate_node_in_context(&node, cell);
        assert!(matches!(
            result,
            crate::calc_result::CalcResult::Number(123.0)
        ));
        let expected = if overflow {
            FrozenEvaluationError::RandomDrawOverflow
        } else {
            FrozenEvaluationError::MissingCellEvaluation
        };
        assert_eq!(actual.frozen_evaluation_result(), Err(expected));
        // A new complete pass starts a fresh stream and clears the prior failure.
        actual.evaluate_frozen().unwrap();
        assert_eq!(number(&actual, "A1").to_bits(), FIRST_A1_BITS);
    }
}

#[test]
fn ordinary_interactive_models_remain_ambient() {
    let mut actual = Model::new_empty("interactive", "en", "UTC", "en").unwrap();
    actual._set("A1", "=RAND()");
    assert_eq!(actual.frozen_evaluation_context(), None);
    assert_eq!(
        actual.evaluate_frozen(),
        Err(FrozenEvaluationError::NotFrozen)
    );
    actual.evaluate();
    assert!((0.0..1.0).contains(&number(&actual, "A1")));
}
