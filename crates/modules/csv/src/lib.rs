//! The `csv` native module (ticket 103): rows of text in, rows of text out.
//!
//! - `Parse { value, header?, separator? }` → `ParseResult { value }`. With a
//!   header (the default) each row is an object keyed by the first line's
//!   names; with `header = false` each row is an array. Every field stays
//!   text: "007" is a code, not a seven, and only the program knows which.
//! - `Stringify { value, header?, separator? }` → `StringifyResult { value }`.
//!   Rows of objects are written under a header line naming every key, in the
//!   order they first appear (`header = false` leaves that line out); rows of
//!   arrays are written as they are. A field is quoted only when it has to be.
//!
//! Quotes, separators and line breaks inside quoted fields follow RFC 4180;
//! a row with more or fewer fields than the first answers an `Exception`
//! naming its line. Stateless (`code_abi.h` item 12).

use code_native::*;
use csv_engine::{ReaderBuilder, WriterBuilder};

#[no_mangle]
pub extern "C" fn code_module_abi_version() -> u32 {
    CODE_ABI_VERSION
}

/// This module keeps no state, so every link of it may share one instance
/// (`code_abi.h` item 12, ticket 177).
#[no_mangle]
pub extern "C" fn code_module_stateless() -> core::ffi::c_int {
    1
}

/// # Safety
/// The host supplies valid initialized ABI values for the duration of dispatch.
#[no_mangle]
pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
    let particle = &*particle;
    guarded(&mut *out, "csv", |out| {
        let outcome = match read_field_str(particle, "_class").unwrap_or("") {
            "Parse" => parse(out, particle),
            "Stringify" => stringify(out, particle),
            _ => {
                null(out);
                Ok(())
            }
        };
        if let Err(message) = outcome {
            exception(out, "csv", &message);
        }
    })
}

/// `header` and `separator`, the two choices both handlers share.
fn options(particle: &CodeValue, class: &str) -> Result<(bool, u8), String> {
    let header = match find_field(particle, "header") {
        None => true,
        Some(v) => read_bool(v).ok_or_else(|| format!("{class} 'header' must be true or false"))?,
    };
    let separator = match find_field(particle, "separator") {
        None => b',',
        Some(v) => match read_str(v).map(str::as_bytes) {
            Some([one]) if one.is_ascii() && !matches!(one, b'"' | b'\n' | b'\r') => *one,
            _ => return Err(format!("{class} 'separator' must be one character, not a quote or a line break")),
        },
    };
    Ok((header, separator))
}

fn parse(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let text = find_field(particle, "value")
        .and_then(read_str)
        .ok_or("Parse requires a text 'value'")?;
    let (header, separator) = options(particle, "Parse")?;
    let mut reader = ReaderBuilder::new()
        .has_headers(header)
        .delimiter(separator)
        .from_reader(text.as_bytes());
    let names: Vec<String> = if header {
        reader
            .headers()
            .map_err(|e| broken(&e))?
            .iter()
            .map(str::to_string)
            .collect()
    } else {
        Vec::new()
    };
    let mut rows = Vec::new();
    for record in reader.records() {
        rows.push(record.map_err(|e| broken(&e))?);
    }
    let keys: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut slots = SlotBuffer::new(rows.len());
    for (i, row) in rows.iter().enumerate() {
        let mut fields = SlotBuffer::new(row.len());
        for (j, field) in row.iter().enumerate() {
            owned_str(fields.slot_mut(j as i64), field);
        }
        if header {
            object_dyn(slots.slot_mut(i as i64), &keys, &mut fields);
        } else {
            array(slots.slot_mut(i as i64), &mut fields);
        }
        fields.release_all();
    }
    make_result(out, c"ParseResult", |slot| array(slot, &mut slots));
    slots.release_all();
    Ok(())
}

/// The reader's error, said with the line it is on.
fn broken(e: &csv_engine::Error) -> String {
    match e.kind() {
        csv_engine::ErrorKind::UnequalLengths {
            pos,
            expected_len,
            len,
        } => {
            let line = pos.as_ref().map(|p| p.line()).unwrap_or(0);
            format!("line {line} has {len} fields, the first has {expected_len}")
        }
        csv_engine::ErrorKind::Utf8 { pos, .. } => {
            let line = pos.as_ref().map(|p| p.line()).unwrap_or(0);
            format!("line {line} is not UTF-8")
        }
        _ => format!("not CSV: {e}"),
    }
}

fn stringify(out: &mut CodeValue, particle: &CodeValue) -> Result<(), String> {
    let rows = find_field(particle, "value")
        .filter(|v| v.tag == CodeTag::Array)
        .ok_or("Stringify requires an array of rows 'value'")?;
    let (header, separator) = options(particle, "Stringify")?;
    let rows: Vec<&CodeValue> = array_elems(rows).collect();
    let of_objects = rows.first().is_some_and(|r| r.tag == CodeTag::Object);
    let mut lines: Vec<Vec<String>> = Vec::new();
    if of_objects {
        let mut names: Vec<&str> = Vec::new();
        for (i, row) in rows.iter().enumerate() {
            if row.tag != CodeTag::Object {
                return Err(format!("row {} is not an object, as the first is", i + 1));
            }
            for (key, _) in object_entries(row) {
                if !names.contains(&key) {
                    names.push(key);
                }
            }
        }
        if header {
            lines.push(names.iter().map(|n| n.to_string()).collect());
        }
        for (i, row) in rows.iter().enumerate() {
            let mut line = Vec::with_capacity(names.len());
            for name in &names {
                line.push(match find_field(row, name) {
                    Some(v) => cell(v, i + 1, name)?,
                    None => String::new(),
                });
            }
            lines.push(line);
        }
    } else {
        for (i, row) in rows.iter().enumerate() {
            if row.tag != CodeTag::Array {
                return Err(format!("row {} is not an array, as the first is", i + 1));
            }
            let mut line = Vec::new();
            for (j, v) in array_elems(row).enumerate() {
                line.push(cell(v, i + 1, &(j + 1).to_string())?);
            }
            lines.push(line);
        }
    }
    let mut writer = WriterBuilder::new()
        .delimiter(separator)
        .flexible(true)
        .from_writer(Vec::new());
    for line in &lines {
        writer.write_record(line).map_err(|e| e.to_string())?;
    }
    let bytes = writer.into_inner().map_err(|e| e.to_string())?;
    let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    make_result(out, c"StringifyResult", |slot| owned_str(slot, &text));
    Ok(())
}

/// One field's text: a number as the language writes it, a bool as
/// true/false, null as nothing. A list or an object has no place in a cell.
fn cell(v: &CodeValue, row: usize, column: &str) -> Result<String, String> {
    match v.tag {
        CodeTag::Str => Ok(read_str(v).unwrap_or("").to_string()),
        CodeTag::Number => Ok(format!("{}", v.number())),
        CodeTag::Bool => Ok(if read_bool(v) == Some(true) { "true" } else { "false" }.to_string()),
        CodeTag::Null => Ok(String::new()),
        _ => Err(format!(
            "row {row}, {column}: a list or an object cannot be one field"
        )),
    }
}
