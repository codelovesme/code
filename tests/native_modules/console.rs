//! Native module: console.so
//! Accepts `Log` particles and prints them.

use code_native::*;

unsafe extern "C" fn handle_log(particle: CodeValue) -> CodeValue {
    let level = read_field_str(&particle, "level");
    let message = read_field_str(&particle, "message");

    if level.is_empty() {
        println!("[Log] {message}");
    } else {
        println!("[{level}] {message}");
    }

    particle
}

code_module! {
    vars: [],
    types: [
        "Log" [
            ("message", "String"),
            ("level", "String"),
        ],
    ],
    handlers: [
        "Log" => handle_log,
    ],
    emissions: [],
}
