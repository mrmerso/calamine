// SPDX-License-Identifier: MIT
//
// Copyright 2016-2026, Johann Tuffe.

//! Tests for reading numbers as `rust_decimal::Decimal`.

#![cfg(feature = "rust_decimal")]

use calamine::rust_decimal::Decimal;
use calamine::{
    open_workbook, open_workbook_auto, Data, DataType, ExcelDateTime, ExcelDateTimeType, Ods,
    Range, RangeDeserializerBuilder, Reader, Xls, Xlsb, Xlsx,
};
use std::fs::File;
use std::io::BufReader;
use std::str::FromStr;

fn wb<R: Reader<BufReader<File>>>(name: &str) -> R {
    let path = format!("{}/tests/{name}", env!("CARGO_MANIFEST_DIR"));
    open_workbook(&path).expect(&path)
}

fn dec(s: &str) -> Data {
    Data::Decimal(Decimal::from_str(s).unwrap())
}

/// Assert the decimal value and its string representation (i.e. the scale).
#[track_caller]
fn assert_decimal(range: &Range<Data>, pos: (u32, u32), expected: &str) {
    match range.get_value(pos) {
        Some(Data::Decimal(d)) => assert_eq!(d.to_string(), expected, "{pos:?}"),
        v => panic!("{pos:?}: expected decimal {expected}, got {v:?}"),
    }
}

/// Values of `decimal.xlsx` and `decimal.ods` read as decimals, in row
/// order. The xlsx has an extra date cell after `1E+300`.
const DECIMALS: [&str; 8] = [
    "0.1",
    "2.9999999999999996",
    // Same f64 as "0.1", must not be collapsed into it.
    "0.10000000000000001",
    // Scale is preserved.
    "1.50",
    "0.00001",
    "123450000000000000000",
    "1234567890123456789012345678",
    // Too many digits: rounded half to even.
    "1.2345678901234567890123456789",
];

#[test]
fn xlsx_decimal() {
    let mut xlsx: Xlsx<_> = wb("decimal.xlsx");
    xlsx.with_decimal_numbers(true);
    let range = xlsx.worksheet_range("Sheet1").unwrap();

    for (row, expected) in DECIMALS.iter().enumerate() {
        assert_decimal(&range, (row as u32, 0), expected);
    }
    // Too large for a decimal.
    assert_eq!(range.get_value((8, 0)), Some(&Data::Float(1e300)));
    // Date format is not affected.
    assert_eq!(
        range.get_value((9, 0)),
        Some(&Data::DateTime(ExcelDateTime::new(
            44197.,
            ExcelDateTimeType::DateTime,
            false
        )))
    );
    assert_decimal(&range, (10, 0), "42");
    assert_decimal(&range, (11, 0), "-0.5");
    // Too small for a decimal: rounded to zero.
    assert_eq!(
        range.get_value((12, 0)),
        Some(&Data::Decimal(Decimal::ZERO))
    );
}

#[test]
fn xlsx_decimal_disabled() {
    let mut xlsx: Xlsx<_> = wb("decimal.xlsx");
    let range = xlsx.worksheet_range("Sheet1").unwrap();
    assert_eq!(range.get_value((0, 0)), Some(&Data::Float(0.1)));
    assert_eq!(range.get_value((3, 0)), Some(&Data::Float(1.5)));
    assert_eq!(range.get_value((8, 0)), Some(&Data::Float(1e300)));

    // The option can be toggled.
    xlsx.with_decimal_numbers(true);
    let range = xlsx.worksheet_range("Sheet1").unwrap();
    assert_decimal(&range, (0, 0), "0.1");
    xlsx.with_decimal_numbers(false);
    let range = xlsx.worksheet_range("Sheet1").unwrap();
    assert_eq!(range.get_value((0, 0)), Some(&Data::Float(0.1)));
}

#[test]
fn xlsx_decimal_cells_reader() {
    let mut xlsx: Xlsx<_> = wb("decimal.xlsx");
    xlsx.with_decimal_numbers(true);
    let mut reader = xlsx.worksheet_cells_reader("Sheet1").unwrap();
    let cell = reader.next_cell().unwrap().unwrap();
    assert_eq!(
        cell.get_value().get_decimal(),
        Decimal::from_str("0.1").ok()
    );
}

#[test]
fn ods_decimal() {
    let mut ods: Ods<_> = wb("decimal.ods");
    // Parsed without the option when opened.
    let range = ods.worksheet_range("Sheet1").unwrap();
    assert_eq!(range.get_value((0, 0)), Some(&Data::Float(0.1)));

    ods.with_decimal_numbers(true);
    let range = ods.worksheet_range("Sheet1").unwrap();
    for (row, expected) in DECIMALS.iter().enumerate() {
        assert_decimal(&range, (row as u32, 0), expected);
    }
    assert_eq!(range.get_value((8, 0)), Some(&Data::Float(1e300)));
    assert_decimal(&range, (9, 0), "42");
    assert_decimal(&range, (10, 0), "-0.5");
    assert_eq!(
        range.get_value((11, 0)),
        Some(&Data::Decimal(Decimal::ZERO))
    );

    let sheets = ods.worksheets();
    assert_eq!(
        sheets[0].1.get_value((1, 0)),
        Some(&dec("2.9999999999999996"))
    );

    ods.with_decimal_numbers(false);
    let range = ods.worksheet_range("Sheet1").unwrap();
    assert_eq!(range.get_value((0, 0)), Some(&Data::Float(0.1)));
}

#[test]
fn xls_decimal() {
    let mut xls: Xls<_> = wb("issues.xls");
    xls.with_decimal_numbers(true);
    let range = xls.worksheet_range("issue2").unwrap();
    assert_eq!(range.get_value((0, 0)), Some(&dec("1")));
    assert_eq!(
        range.get_value((0, 1)),
        Some(&Data::String("a".to_string()))
    );
    assert_eq!(range.get_value((2, 0)), Some(&dec("3")));

    let sheets = xls.worksheets();
    let (_, range) = sheets.iter().find(|(name, _)| name == "issue2").unwrap();
    assert_eq!(range.get_value((1, 0)), Some(&dec("2")));

    let mut xls: Xls<_> = wb("xls_wrong_decimals.xls");
    xls.with_decimal_numbers(true);
    let range = xls.worksheet_range_at(0).unwrap().unwrap();
    assert_decimal(&range, (0, 0), "0.525625");

    // Dates are not affected.
    let mut xls: Xls<_> = wb("date.xls");
    xls.with_decimal_numbers(true);
    let range = xls.worksheet_range_at(0).unwrap().unwrap();
    assert!(range.get_value((0, 0)).unwrap().is_datetime());
}

#[test]
fn xlsb_decimal() {
    let mut xlsb: Xlsb<_> = wb("issues.xlsb");
    xlsb.with_decimal_numbers(true);
    let range = xlsb.worksheet_range("issue2").unwrap();
    assert_eq!(range.get_value((0, 0)), Some(&dec("1")));
    assert_eq!(
        range.get_value((0, 1)),
        Some(&Data::String("a".to_string()))
    );
    assert_eq!(range.get_value((2, 0)), Some(&dec("3")));

    // Dates are not affected.
    let mut xlsb: Xlsb<_> = wb("date.xlsb");
    xlsb.with_decimal_numbers(true);
    let range = xlsb.worksheet_range_at(0).unwrap().unwrap();
    assert!(range.get_value((0, 0)).unwrap().is_datetime());
}

#[test]
fn auto_decimal() {
    let path = format!("{}/tests/decimal.xlsx", env!("CARGO_MANIFEST_DIR"));
    let mut sheets = open_workbook_auto(path).unwrap();
    sheets.with_decimal_numbers(true);
    let range = sheets.worksheet_range("Sheet1").unwrap();
    assert_decimal(&range, (2, 0), "0.10000000000000001");
}

#[test]
fn data_type_decimal() {
    let d = dec("1.50");
    assert!(d.is_decimal());
    assert!(!d.is_float());
    assert_eq!(d.get_decimal(), Decimal::from_str("1.50").ok());
    assert_eq!(d.get_float(), None);
    assert_eq!(d.as_f64(), Some(1.5));
    assert_eq!(d.as_i64(), Some(1));
    assert_eq!(d.as_string(), Some("1.50".to_string()));
    assert_eq!(d.to_string(), "1.50");

    assert_eq!(Data::Float(0.1).as_decimal(), Decimal::from_str("0.1").ok());
    assert_eq!(Data::Int(7).as_decimal(), Some(Decimal::from(7)));
    assert_eq!(
        Data::String("1.2E-3".to_string()).as_decimal(),
        Decimal::from_str("0.0012").ok()
    );
    assert_eq!(Data::Bool(true).as_decimal(), None);
    assert_eq!(Data::from(Decimal::ONE), Data::Decimal(Decimal::ONE));
}

#[test]
fn deserialize_decimal() {
    #[derive(serde_derive::Deserialize)]
    struct Row {
        value: f64,
        text: String,
        int: i64,
    }

    let range = Range::from_sparse(vec![
        calamine::Cell::new((0, 0), Data::String("value".to_string())),
        calamine::Cell::new((0, 1), Data::String("text".to_string())),
        calamine::Cell::new((0, 2), Data::String("int".to_string())),
        calamine::Cell::new((1, 0), dec("0.10000000000000001")),
        calamine::Cell::new((1, 1), dec("0.10000000000000001")),
        calamine::Cell::new((1, 2), dec("42.9")),
    ]);
    let rows: Vec<Row> = RangeDeserializerBuilder::new()
        .from_range(&range)
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(rows[0].value, 0.1);
    assert_eq!(rows[0].text, "0.10000000000000001");
    assert_eq!(rows[0].int, 42);

    // Out of range integers are an error rather than wrapping.
    #[derive(Debug, serde_derive::Deserialize)]
    #[allow(dead_code)]
    struct Small {
        value: u8,
    }
    let range = Range::from_sparse(vec![
        calamine::Cell::new((0, 0), Data::String("value".to_string())),
        calamine::Cell::new((1, 0), dec("300")),
    ]);
    let mut iter = RangeDeserializerBuilder::new()
        .from_range::<_, Small>(&range)
        .unwrap();
    assert!(iter.next().unwrap().is_err());
}
