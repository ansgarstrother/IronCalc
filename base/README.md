# IronCalc Base

[![Crates.io][crates-badge]][crates-url]
[![MIT licensed][mit-badge]][mit-url]

[crates-badge]: https://img.shields.io/crates/v/ironcalc_base.svg
[crates-url]: https://crates.io/crates/ironcalc_base
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: https://github.com/ironcalc/ironcalc/blob/master/LICENSE


## About

IronCalc Base is the engine of the IronCalc ecosystem


## Build
To build the library

```bash
$ cargo build --release
```

## Tests

To run the tests:

```bash
$ cargo tests
```







## Explicit volatile evaluation

Frozen models use an immutable UTC timestamp and a 32-byte seed supplied by the
host. Construct one with Model::new_empty_with_frozen_context,
Model::from_workbook_with_frozen_context, or
Model::from_bytes_with_frozen_context, then call evaluate_frozen.

    use ironcalc_base::{FrozenEvaluationContext, Model};

    let context = FrozenEvaluationContext::new(1_679_319_865_208, [7; 32])?;
    let mut model =
        Model::new_empty_with_frozen_context("model", "en", "UTC", "en", context)?;
    model.set_user_input(0, 1, 1, "=NOW()+RAND()".to_owned())?;
    model.evaluate_frozen()?;

NOW and TODAY use the supplied clock with the existing timezone and second
precision rules, flooring pre-epoch timestamps to their containing second.
RAND and RANDBETWEEN draw from a versioned SHA-256 stream keyed
by seed, cell coordinates, and that cell's executed draw index. A new evaluation
pass resets the streams; lazy branches and cached references keep the ordinary
engine semantics. Existing interactive constructors continue to use ambient
time and randomness.

The context is runtime state and is not serialized with a workbook. Loading
cached values does not establish that they were evaluated with the context.
Operational frozen-evaluation errors are returned even if a formula handles an
error value. The host must bind the exact context to its own source, viewer and
evaluation identity; this API grants no authority and creates no native receipt.
It does not promise canonical XLSX bytes or identical results for every numeric
function on every platform.
