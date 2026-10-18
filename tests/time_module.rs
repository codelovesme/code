#![cfg(feature = "llvm")]
#[path = "support/utility_modules.rs"]
mod utilities;
#[test]
fn clock_queries_match_core_on_native_and_browser() {
    utilities::fixture("time", "time_queries.code");
}

#[test]
fn calendar_days_agree_on_native_and_browser() {
    utilities::fixture("time", "time_calendar.code");
}
