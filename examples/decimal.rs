// SPDX-License-Identifier: MIT
//
// Copyright 2016-2026, Johann Tuffe.

//! An example of reading numbers as `rust_decimal::Decimal` instead of `f64`.
//!
//! Requires the `rust_decimal` feature:
//!
//! ```bash
//! cargo run --example decimal --features rust_decimal
//! ```
//!
//! Xlsx and ods files store numbers as text. With
//! [`Reader::with_decimal_numbers()`] the text is converted directly into a
//! `Decimal`, so values like `0.10000000000000001`, which are the same number
//! as `0.1` once read as `f64`, are kept as written.

use calamine::{open_workbook, Data, Reader, Xlsx};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = format!("{}/tests/decimal.xlsx", env!("CARGO_MANIFEST_DIR"));
    let mut workbook: Xlsx<_> = open_workbook(path)?;
    workbook.with_decimal_numbers(true);
    let range = workbook.worksheet_range("Sheet1")?;

    for row in range.rows() {
        match &row[0] {
            Data::Decimal(d) => println!("decimal: {d}"),
            // Numbers too large for a decimal are still read as floats.
            Data::Float(f) => println!("float: {f:e}"),
            other => println!("other: {other:?}"),
        }
    }

    Ok(())
}
