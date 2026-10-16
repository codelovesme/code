//! Stateless array utilities; README.md defines fields, equality and ordering.
#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
    use code_native::*;
    use std::{cmp::Ordering, ffi::CStr};

    #[no_mangle]
    pub extern "C" fn code_module_abi_version() -> u32 {
        CODE_ABI_VERSION
    }

    /// # Safety
    /// The host supplies valid initialized ABI values for the duration of dispatch.
    #[no_mangle]
    pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
        guarded(&mut *out, "arrays", |out| {
            if let Err(message) = dispatch(out, &*particle) {
                exception(out, "arrays", &message);
            }
        });
    }

    fn array_result(out: &mut CodeValue, class: &'static CStr, items: &[&CodeValue]) {
        let mut slots = SlotBuffer::new(items.len());
        for (i, item) in items.iter().enumerate() {
            copy(slots.slot_mut(i as i64), item);
        }
        make_result(out, class, |slot| {
            array(slot, &mut slots);
            slots.release_all();
        });
    }

    fn bound(p: &CodeValue, field: &str, default: i64, len: i64) -> Result<usize, String> {
        let n = match find_field(p, field) {
            None => default,
            Some(v) => read_number(v)
                .filter(|n| n.is_finite() && n.fract() == 0.0 && n.abs() <= 9_007_199_254_740_991.0)
                .ok_or_else(|| format!("Slice requires a safe integer '{field}' field"))?
                as i64,
        };
        Ok(if n < 0 { (len + n).max(0) } else { n.min(len) } as usize)
    }

    fn dispatch(out: &mut CodeValue, p: &CodeValue) -> Result<(), String> {
        let name = read_field_str(p, "_class").unwrap_or("");
        if ![
            "Length", "Contains", "IndexOf", "Reverse", "Slice", "Unique", "Sort", "Sum", "Min",
            "Max",
        ]
        .contains(&name)
        {
            null(out);
            return Ok(());
        }
        let value = find_field(p, "value")
            .filter(|v| v.tag == CodeTag::Array)
            .ok_or_else(|| format!("{name} requires an array 'value'"))?;
        if name == "Length" {
            make_result(out, c"LengthResult", |slot| {
                number(slot, value.len() as f64)
            });
            return Ok(());
        }
        let mut items: Vec<&CodeValue> = array_elems(value).collect();
        match name {
            "Contains" | "IndexOf" => {
                // Missing fields read as null, just like Code field access and Echo.
                let mut absent = CodeValue::zeroed();
                null(&mut absent);
                let item = find_field(p, "item").unwrap_or(&absent);
                let found = items.iter().position(|v| values_equal(v, item));
                if name == "Contains" {
                    make_result(out, c"ContainsResult", |slot| {
                        boolean(slot, found.is_some())
                    });
                } else {
                    make_result(out, c"IndexOfResult", |slot| {
                        number(slot, found.map(|i| i as f64).unwrap_or(-1.0))
                    });
                }
            }
            "Reverse" => {
                items.reverse();
                array_result(out, c"ReverseResult", &items);
            }
            "Slice" => {
                let start = bound(p, "start", 0, items.len() as i64)?;
                let end = bound(p, "end", items.len() as i64, items.len() as i64)?;
                array_result(out, c"SliceResult", &items[start..end.max(start)]);
            }
            "Unique" => {
                let mut unique: Vec<&CodeValue> = Vec::new();
                for item in items {
                    if !unique.iter().any(|v| values_equal(v, item)) {
                        unique.push(item);
                    }
                }
                array_result(out, c"UniqueResult", &unique);
            }
            "Sort" => sort(out, p, items)?,
            _ => {
                let mut answer: Option<f64> = if name == "Sum" { Some(0.0) } else { None };
                for item in items {
                    let n = read_number(item)
                        .filter(|n| n.is_finite())
                        .ok_or_else(|| format!("{name} requires an array of finite numbers"))?;
                    answer = Some(match (name, answer) {
                        ("Sum", Some(a)) => a + n,
                        ("Min", Some(a)) => a.min(n),
                        ("Max", Some(a)) => a.max(n),
                        _ => n,
                    });
                }
                if answer.is_some_and(|n| !n.is_finite()) {
                    return Err(format!("{name} result must be finite"));
                }
                let class = match name {
                    "Sum" => c"SumResult",
                    "Min" => c"MinResult",
                    _ => c"MaxResult",
                };
                make_result(out, class, |slot| match answer {
                    Some(n) => number(slot, if n == 0.0 { 0.0 } else { n }),
                    None => null(slot),
                });
            }
        }
        Ok(())
    }

    fn sort(out: &mut CodeValue, p: &CodeValue, items: Vec<&CodeValue>) -> Result<(), String> {
        let by = match find_field(p, "by") {
            None => None,
            Some(v) => Some(read_str(v).ok_or("Sort requires a string 'by' field")?),
        };
        let descending = match find_field(p, "descending") {
            None => false,
            Some(v) => read_bool(v).ok_or("Sort requires a boolean 'descending' field")?,
        };
        let mut keyed = Vec::with_capacity(items.len());
        let mut kind = None;
        for item in items {
            let key = match by {
                None => item,
                Some(field) => find_field(item, field)
                    .ok_or("Sort requires object items with the 'by' field")?,
            };
            if !(key.tag == CodeTag::Str || read_number(key).is_some_and(f64::is_finite)) {
                return Err("Sort requires finite number or string keys".into());
            }
            if kind.is_some_and(|tag| tag != key.tag) {
                return Err("Sort requires keys of one type".into());
            }
            kind = Some(key.tag);
            keyed.push((item, key));
        }
        // Validation precedes sorting so the comparator is total. sort_by is
        // stable; reverse only comparison, preserving equal-key input order.
        keyed.sort_by(|(_, a), (_, b)| {
            let order = match (read_number(a), read_number(b)) {
                (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
                _ => read_str(a).unwrap().cmp(read_str(b).unwrap()),
            };
            if descending {
                order.reverse()
            } else {
                order
            }
        });
        let items: Vec<_> = keyed.into_iter().map(|(item, _)| item).collect();
        array_result(out, c"SortResult", &items);
        Ok(())
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn abi_rejects_non_finite_sort_keys_and_aggregates() {
            for class in [c"Sum", c"Min", c"Max", c"Sort"] {
                for n in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                    let mut elements = SlotBuffer::new(1);
                    number(elements.slot_mut(0), n);
                    let mut fields = SlotBuffer::new(2);
                    borrowed_str(fields.slot_mut(0), class);
                    array(fields.slot_mut(1), &mut elements);
                    elements.release_all();
                    let mut particle = CodeValue::zeroed();
                    object(&mut particle, &[c"_class", c"value"], &mut fields);
                    fields.release_all();
                    let mut answer = CodeValue::zeroed();
                    // SAFETY: initialized, owned ABI values.
                    unsafe {
                        code_module_dispatch(&mut answer, &particle);
                    }
                    assert_eq!(read_field_str(&answer, "_class"), Some("Exception"));
                    let name = class.to_str().unwrap();
                    let message = if name == "Sort" {
                        "Sort requires finite number or string keys".to_owned()
                    } else {
                        format!("{name} requires an array of finite numbers")
                    };
                    assert_eq!(read_field_str(&answer, "message"), Some(message.as_str()));
                    release(&mut answer);
                    release(&mut particle);
                }
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
include!("../../browser_half.rs");
#[cfg(target_arch = "wasm32")]
browser_half!(
    "arrays",
    arrays_code_module_abi_version,
    arrays_code_module_dispatch
);
