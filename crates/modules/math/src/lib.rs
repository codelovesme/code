//! Stateless numeric utilities. See ../README.md for contracts and errors.
#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
    use code_native::*;
    use std::ffi::CStr;

    #[no_mangle]
    pub extern "C" fn code_module_abi_version() -> u32 {
        CODE_ABI_VERSION
    }

    /// # Safety
    /// Both pointers must be valid ABI values for the duration of dispatch.
    #[no_mangle]
    pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
        let particle = &*particle;
        guarded(&mut *out, "math", |out| {
            let class = read_field_str(particle, "_class").unwrap_or("");
            let result = match class {
                "Double" | "Sum" | "Floor" | "Ceil" | "Round" | "Trunc" | "Div" | "Mod" | "Abs"
                | "Min" | "Max" | "Parse" | "Sqrt" | "Pow" | "Sin" | "Cos" | "Ln" | "Exp" => {
                    evaluate(class, particle)
                }
                _ => {
                    null(out);
                    return;
                }
            };
            match result {
                Ok(value) => {
                    make_result(out, result_class(class), |slot| {
                        number(slot, if value == 0.0 { 0.0 } else { value })
                    });
                }
                Err(message) => exception(out, "math", &message),
            }
        });
    }

    fn result_class(class: &str) -> &'static CStr {
        match class {
            "Double" => c"DoubleResult",
            "Sum" => c"SumResult",
            "Floor" => c"FloorResult",
            "Ceil" => c"CeilResult",
            "Round" => c"RoundResult",
            "Trunc" => c"TruncResult",
            "Div" => c"DivResult",
            "Mod" => c"ModResult",
            "Abs" => c"AbsResult",
            "Min" => c"MinResult",
            "Max" => c"MaxResult",
            "Parse" => c"ParseResult",
            "Sqrt" => c"SqrtResult",
            "Pow" => c"PowResult",
            "Sin" => c"SinResult",
            "Cos" => c"CosResult",
            "Ln" => c"LnResult",
            "Exp" => c"ExpResult",
            _ => unreachable!(),
        }
    }

    fn operand(particle: &CodeValue, class: &str, field: &str) -> Result<f64, String> {
        let value = find_field(particle, field)
            .and_then(read_number)
            .ok_or_else(|| format!("{class} requires a numeric '{field}'"))?;
        if !value.is_finite() {
            return Err(format!("{class} requires a numeric '{field}'"));
        }
        Ok(value)
    }

    fn evaluate(class: &str, particle: &CodeValue) -> Result<f64, String> {
        let value = if class == "Parse" {
            let text = find_field(particle, "text")
                .and_then(read_str)
                .ok_or("Parse requires a string 'text'")?;
            if !decimal(text) {
                return Err("Parse requires decimal text".into());
            }
            text.parse::<f64>()
                .map_err(|_| "Parse requires decimal text")?
        } else if class == "Sum" {
            let items = find_field(particle, "value")
                .filter(|v| v.tag == CodeTag::Array)
                .ok_or("Sum requires an array 'value'")?;
            let mut sum = 0.0;
            for item in array_elems(items) {
                let n = read_number(item).ok_or("Sum requires an array of numbers")?;
                if !n.is_finite() {
                    return Err("Sum requires an array of numbers".into());
                }
                sum += n;
            }
            sum
        } else {
            let n = operand(particle, class, "value")?;
            match class {
                "Double" => n * 2.0,
                "Floor" => n.floor(),
                "Ceil" => n.ceil(),
                "Round" => n.round(),
                "Trunc" => n.trunc(),
                "Abs" => n.abs(),
                "Sqrt" => {
                    if n < 0.0 {
                        return Err("Sqrt requires a non-negative 'value'".into());
                    }
                    n.sqrt()
                }
                "Ln" => {
                    if n <= 0.0 {
                        return Err("Ln requires a positive 'value'".into());
                    }
                    n.ln()
                }
                "Exp" => n.exp(),
                "Sin" => n.sin(),
                "Cos" => n.cos(),
                "Pow" => n.powf(operand(particle, class, "exponent")?),
                "Min" => n.min(operand(particle, class, "other")?),
                "Max" => n.max(operand(particle, class, "other")?),
                "Div" | "Mod" => {
                    let divisor = operand(particle, class, "by")?;
                    if divisor == 0.0 {
                        return Err(format!("{class} requires a non-zero 'by'"));
                    }
                    if class == "Div" {
                        (n / divisor).floor()
                    } else {
                        let remainder = n % divisor;
                        if remainder != 0.0
                            && remainder.is_sign_negative() != divisor.is_sign_negative()
                        {
                            remainder + divisor
                        } else {
                            remainder
                        }
                    }
                }
                _ => unreachable!(),
            }
        };
        if !value.is_finite() {
            return Err(format!("{class} result must be finite"));
        }
        Ok(value)
    }

    // Same strict ASCII decimal grammar as page.mjs, including exponent form.
    fn decimal(text: &str) -> bool {
        let bytes = text.as_bytes();
        let mut i = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
        let mut digits = 0;
        while bytes.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
            digits += 1;
        }
        if bytes.get(i) == Some(&b'.') {
            i += 1;
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
                digits += 1;
            }
        }
        if digits == 0 {
            return false;
        }
        if matches!(bytes.get(i), Some(b'e' | b'E')) {
            i += 1;
            if matches!(bytes.get(i), Some(b'+' | b'-')) {
                i += 1;
            }
            let start = i;
            while bytes.get(i).is_some_and(u8::is_ascii_digit) {
                i += 1;
            }
            if i == start {
                return false;
            }
        }
        i == bytes.len()
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn native_dispatch_refuses_non_finite_operands_and_overflow() {
            for (class, n, message) in [
                (c"Floor", f64::NAN, "Floor requires a numeric 'value'"),
                (
                    c"Double",
                    f64::INFINITY,
                    "Double requires a numeric 'value'",
                ),
                (c"Double", f64::MAX, "Double result must be finite"),
            ] {
                let mut fields = SlotBuffer::new(2);
                borrowed_str(fields.slot_mut(0), class);
                number(fields.slot_mut(1), n);
                let mut particle = CodeValue::zeroed();
                object(&mut particle, &[c"_class", c"value"], &mut fields);
                fields.release_all();
                let mut answer = CodeValue::zeroed();
                // SAFETY: both arguments are initialized ABI values we own.
                unsafe {
                    code_module_dispatch(&mut answer, &particle);
                }
                assert_eq!(read_field_str(&answer, "_class"), Some("Exception"));
                assert_eq!(read_field_str(&answer, "message"), Some(message));
                release(&mut answer);
                release(&mut particle);
            }
        }

        #[test]
        fn strict_decimal_grammar() {
            for text in ["0", "-0", "+.5", "1.", "-12.25e+3", "1e-999"] {
                assert!(decimal(text), "{text}");
            }
            for text in [
                "", "+", ".", " 1", "1 ", "1_000", "0xff", "NaN", "Infinity", "1e", "1e+", "１２",
            ] {
                assert!(!decimal(text), "{text}");
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
include!("../../browser_half.rs");
#[cfg(target_arch = "wasm32")]
browser_half!(
    "math",
    math_code_module_abi_version,
    math_code_module_dispatch
);
