#![cfg(feature = "native-modules")]
#[path = "support/modules.rs"]
mod modules;
use code::{native::NativeModule, value::Value};
use std::rc::Rc;

fn object(fields: &[(&str, Value)]) -> Value {
    Value::Object(Rc::new(
        fields
            .iter()
            .map(|(k, v)| ((*k).into(), v.clone()))
            .collect(),
    ))
}
fn particle(class: &str, value: Value) -> Value {
    object(&[("_class", Value::Str(class.into())), ("value", value)])
}

#[test]
fn metadata_is_opt_in_and_ordinary_fallback_keeps_borrowed_input_alive() {
    let module = NativeModule::open(modules::build_so("test_borrowed").to_str().unwrap()).unwrap();
    let array = Value::Array(Rc::new(vec![Value::Null; 100_000]));
    assert_eq!(
        module
            .dispatch(&particle("Inspect", array.clone()))
            .unwrap(),
        object(&[
            ("_class", Value::Str("Inspected".into())),
            ("value", Value::Number(100_000.0))
        ])
    );
    assert_eq!(
        module.dispatch(&particle("Length", array)).unwrap(),
        Value::Number(777.0)
    );
    let value = object(&[(
        "nested",
        Value::Array(Rc::new(vec![Value::Str("é😀".into())])),
    )]);
    for _ in 0..100 {
        assert_eq!(
            module
                .dispatch(&particle("Fallback", value.clone()))
                .unwrap(),
            value
        );
    }
    let first = object(&[
        ("_class", Value::Str("Inspect".into())),
        ("value", Value::Str("é😀".into())),
        ("value", Value::Null),
    ]);
    assert_eq!(
        module.dispatch(&first).unwrap(),
        object(&[
            ("_class", Value::Str("Inspected".into())),
            ("value", Value::Number(6.0))
        ])
    );
    let old = NativeModule::open(modules::build_so("test_math").to_str().unwrap()).unwrap();
    assert_eq!(
        old.dispatch(&particle("Double", Value::Number(2.0)))
            .unwrap(),
        object(&[
            ("_class", Value::Str("DoubleResult".into())),
            ("value", Value::Number(4.0))
        ])
    );
    assert_eq!(
        old.dispatch(&particle("Length", Value::Array(Rc::new(vec![]))))
            .unwrap(),
        Value::Null
    );
}

#[test]
fn string_fast_path_keeps_the_native_interior_nul_transport_rule() {
    let module = NativeModule::open(modules::build_so("strings").to_str().unwrap()).unwrap();
    assert_eq!(
        module
            .dispatch(&particle("Length", Value::Str("a\0b".into())))
            .unwrap(),
        object(&[
            ("_class", Value::Str("LengthResult".into())),
            ("value", Value::Number(0.0))
        ])
    );
    assert_eq!(
        module
            .dispatch(&particle("Length\0ignored", Value::Str("abc".into())))
            .unwrap(),
        Value::Null
    );
    let arrays = NativeModule::open(modules::build_so("arrays").to_str().unwrap()).unwrap();
    let duplicate = object(&[
        ("_class", Value::Str("Length".into())),
        ("value", Value::Array(Rc::new(vec![]))),
        ("value", Value::Null),
    ]);
    assert_eq!(
        arrays.dispatch(&duplicate).unwrap(),
        object(&[
            ("_class", Value::Str("LengthResult".into())),
            ("value", Value::Number(0.0))
        ])
    );
}
