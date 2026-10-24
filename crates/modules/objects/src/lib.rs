//! Stateless helpers for an object used as a map (ticket 102); README.md
//! defines the fields. Reading a key by name (`m[k]`), setting one
//! (`m + { "$k" = v }`) and walking them (`loop k, v over m`) are the
//! language's own; what it could not say is whether a key is there at all
//! when its value is null, how to drop one, and its keys or values as lists.
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
    /// The host supplies valid initialized ABI values for the duration of dispatch.
    #[no_mangle]
    pub unsafe extern "C" fn code_module_dispatch(out: *mut CodeValue, particle: *const CodeValue) {
        guarded(&mut *out, "objects", |out| {
            if let Err(message) = dispatch(out, &*particle) {
                exception(out, "objects", &message);
            }
        });
    }

    fn dispatch(out: &mut CodeValue, p: &CodeValue) -> Result<(), String> {
        let name = read_field_str(p, "_class").unwrap_or("");
        if !["Keys", "Values", "Has", "Delete"].contains(&name) {
            null(out);
            return Ok(());
        }
        let value = find_field(p, "value")
            .filter(|v| v.tag == CodeTag::Object)
            .ok_or_else(|| format!("{name} requires an object 'value'"))?;
        let entries: Vec<(&str, &CodeValue)> = object_entries(value).collect();
        match name {
            "Keys" => {
                let mut slots = SlotBuffer::new(entries.len());
                for (i, (key, _)) in entries.iter().enumerate() {
                    owned_str(slots.slot_mut(i as i64), key);
                }
                make_result(out, c"KeysResult", |slot| {
                    array(slot, &mut slots);
                    slots.release_all();
                });
            }
            "Values" => {
                let mut slots = SlotBuffer::new(entries.len());
                for (i, (_, item)) in entries.iter().enumerate() {
                    copy(slots.slot_mut(i as i64), item);
                }
                make_result(out, c"ValuesResult", |slot| {
                    array(slot, &mut slots);
                    slots.release_all();
                });
            }
            _ => {
                let key = find_field(p, "key")
                    .and_then(read_str)
                    .ok_or_else(|| format!("{name} requires a text 'key'"))?;
                if name == "Has" {
                    let found = entries.iter().any(|(k, _)| *k == key);
                    make_result(out, c"HasResult", |slot| boolean(slot, found));
                } else {
                    let kept: Vec<&(&str, &CodeValue)> =
                        entries.iter().filter(|(k, _)| *k != key).collect();
                    let names: Vec<&str> = kept.iter().map(|(k, _)| *k).collect();
                    let mut slots = SlotBuffer::new(kept.len());
                    for (i, (_, item)) in kept.iter().enumerate() {
                        copy(slots.slot_mut(i as i64), item);
                    }
                    make_result(out, c"DeleteResult", |slot| {
                        object_dyn(slot, &names, &mut slots);
                        slots.release_all();
                    });
                }
            }
        }
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
include!("../../browser_half.rs");
#[cfg(target_arch = "wasm32")]
browser_half!(
    "objects",
    objects_code_module_abi_version,
    objects_code_module_dispatch
);
