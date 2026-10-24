//! Stateless clock queries, separate from timer's pending callbacks.
#![cfg_attr(target_arch = "wasm32", no_std)]

#[cfg(not(target_arch = "wasm32"))]
mod machine {
    use code_native::*;

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
    /// Both pointers must be valid ABI values for the duration of dispatch.
    #[no_mangle]
    pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
        let particle = &*particle;
        guarded(&mut *out, "time", |out| {
            match read_field_str(particle, "_class").unwrap_or("") {
                "Timestamp" => make_result(out, c"TimestampResult", |v| number(v, timestamp())),
                "TimezoneOffset" => {
                    make_result(out, c"TimezoneOffsetResult", |v| number(v, offset()))
                }
                "DaysFromDate" => match days_from_date(particle) {
                    Ok(days) => make_result(out, c"DaysFromDateResult", |v| number(v, days as f64)),
                    Err(message) => exception(out, "time", &message),
                },
                "DateFromDays" => match whole(particle, "days", "DateFromDays", 1e11) {
                    Ok(days) => {
                        let (year, month, day) = date_from_days(days);
                        let mut buf = SlotBuffer::new(5);
                        borrowed_str(buf.slot_mut(0), c"DateFromDaysResult");
                        number(buf.slot_mut(1), year as f64);
                        number(buf.slot_mut(2), month as f64);
                        number(buf.slot_mut(3), day as f64);
                        number(buf.slot_mut(4), (days + 4).rem_euclid(7) as f64);
                        object(
                            out,
                            &[c"_class", c"year", c"month", c"day", c"weekday"],
                            &mut buf,
                        );
                        buf.release_all();
                    }
                    Err(message) => exception(out, "time", &message),
                },
                _ => null(out),
            }
        });
    }

    /// A whole-number field within ±`limit`, or the handler's refusal.
    fn whole(particle: &CodeValue, name: &str, class: &str, limit: f64) -> Result<i64, String> {
        match read_field_number(particle, name) {
            Some(n) if n.is_finite() && n.fract() == 0.0 && n.abs() <= limit => Ok(n as i64),
            _ => Err(format!("{class} requires a whole-number '{name}'")),
        }
    }

    /// Days since 1970-01-01 in the proleptic Gregorian calendar — Howard
    /// Hinnant's days_from_civil. A day past its month's end rolls into the
    /// next month (February 30 is March 1 or 2), as the apps' copies did.
    fn days_from_date(particle: &CodeValue) -> Result<i64, String> {
        let y = whole(particle, "year", "DaysFromDate", 1e8)?;
        let m = whole(particle, "month", "DaysFromDate", 1e8)?;
        let d = whole(particle, "day", "DaysFromDate", 1e8)?;
        if !(1..=12).contains(&m) {
            return Err("DaysFromDate requires a 'month' from 1 to 12".into());
        }
        if !(1..=31).contains(&d) {
            return Err("DaysFromDate requires a 'day' from 1 to 31".into());
        }
        let y = if m <= 2 { y - 1 } else { y };
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        Ok(era * 146097 + doe - 719468)
    }

    /// The date `days` after 1970-01-01 — Hinnant's civil_from_days.
    fn date_from_days(days: i64) -> (i64, i64, i64) {
        let z = days + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        (yoe + era * 400 + i64::from(m <= 2), m, d)
    }

    fn timestamp() -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as f64
    }

    #[cfg(unix)]
    fn offset() -> f64 {
        let now = timestamp() as libc::time_t;
        // SAFETY: owned output storage and a live time_t match localtime_r's contract.
        let mut local: libc::tm = unsafe { std::mem::zeroed() };
        if unsafe { libc::localtime_r(&now, &mut local) }.is_null() {
            return 0.0;
        }
        local.tm_gmtoff as f64 / 60.0
    }
    #[cfg(not(unix))]
    fn offset() -> f64 {
        0.0
    }
}

#[cfg(target_arch = "wasm32")]
include!("../../browser_half.rs");
#[cfg(target_arch = "wasm32")]
browser_half!(
    "time",
    time_code_module_abi_version,
    time_code_module_dispatch
);
