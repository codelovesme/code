//! `test_helper` — Same functionality as test_strings.rs but using the
//! code-native helper crate to eliminate ABI boilerplate.
//!
//! Exports:
//!   Variables: VERSION (String), MAX_LEN (Number)
//!   Types:     Message { text: String, urgent: Boolean }
//!              FireLog { message: String }
//!              FireException { message: String }
//!              Log { source: String, level: String, message: String }
//!              Exception { source: String, message: String }
//!   Handlers:  Message (uppercases text if urgent)
//!              FireLog (emits a Log particle)
//!              FireException (emits an Exception particle)
//!   Emissions: Log => base, Exception => base

use code_native::*;

// -----------------------------------------------------------------------
// Handlers
// -----------------------------------------------------------------------

unsafe extern "C" fn handle_message(particle: CodeValue) -> CodeValue {
    let text = read_field_str(&particle, "text");
    let urgent = read_field_bool(&particle, "urgent");

    let processed = if urgent {
        text.to_uppercase()
    } else {
        text.to_string()
    };

    code_object(vec![
        code_field("_class", code_string("Message")),
        code_field("text", code_string(&processed)),
        code_field("urgent", code_boolean(urgent)),
        code_field("processed", code_boolean(true)),
    ])
}

/// Handler for FireLog particles — emits a Log particle.
unsafe extern "C" fn handle_fire_log(particle: CodeValue) -> CodeValue {
    let msg = read_field_str(&particle, "message");
    code_emit_log("test_helper", "Info", &msg);
    code_null()
}

/// Handler for FireException particles — emits an Exception particle.
unsafe extern "C" fn handle_fire_exception(particle: CodeValue) -> CodeValue {
    let msg = read_field_str(&particle, "message");
    code_emit_exception("test_helper", &msg);
    code_null()
}

// -----------------------------------------------------------------------
// Module declaration
// -----------------------------------------------------------------------

code_module! {
    vars: [
        "VERSION" => code_string("1.0.0"),
        "MAX_LEN" => code_number(1024.0),
    ],
    types: [
        "Message" [
            ("text", "String"),
            ("urgent", "Boolean"),
        ],
        "FireLog" [
            ("message", "String"),
        ],
        "FireException" [
            ("message", "String"),
        ],
        "Log" [
            ("source", "String"),
            ("level", "String"),
            ("message", "String"),
        ],
        "Exception" [
            ("source", "String"),
            ("message", "String"),
        ],
    ],
    handlers: [
        "Message" => handle_message,
        "FireLog" => handle_fire_log,
        "FireException" => handle_fire_exception,
    ],
    emissions: [
        "Log" => "base",
        "Exception" => "base",
    ],
}
