//! Optional, synchronous metadata-only dispatch (ABI 2, extension version 1).
//! No pointer obtained here may be retained after dispatch or passed to a
//! CodeValue constructor/release function. Hosts may decline this extension;
//! modules must retain their ordinary code_module_dispatch implementation.
use core::{ffi::c_void, marker::PhantomData};
#[path = "../vendor/borrowed_abi.rs"]
mod abi;
pub use abi::{CodeBorrowedApi, CodeValueInfo};

/// A read-only view, valid only for the synchronous call that supplied it.
#[derive(Clone, Copy)]
pub struct BorrowedValue<'a> {
    api: &'a CodeBorrowedApi,
    value: *const c_void,
    _borrow: PhantomData<&'a c_void>,
}

impl<'a> BorrowedValue<'a> {
    /// # Safety
    /// The API must have a valid version prefix. For version 1, the full API,
    /// value, all returned fields/bytes and callbacks must remain valid and
    /// immutable for 'a, within this dispatch, on the calling thread. String
    /// bytes must be valid UTF-8, as required by CodeValueInfo's contract.
    pub unsafe fn from_raw(api: *const CodeBorrowedApi, value: *const c_void) -> Option<Self> {
        if api.is_null() || value.is_null() {
            return None;
        }
        // Read only the stable prefix before assuming this version's layout.
        if api.cast::<u32>().read() != 1 {
            return None;
        }
        let api = &*api;
        Some(Self {
            api,
            value,
            _borrow: PhantomData,
        })
    }

    pub fn info(self) -> CodeValueInfo {
        // SAFETY: guaranteed by from_raw, including pointers returned by field.
        unsafe { (self.api.inspect)(self.value) }
    }

    pub fn field(self, key: &str) -> Option<Self> {
        // SAFETY: callback borrows the key only during this call.
        let value = unsafe { (self.api.field)(self.value, key.as_ptr(), key.len() as u64) };
        if value.is_null() {
            None
        } else {
            Some(Self { value, ..self })
        }
    }

    pub fn text(self) -> Option<&'a str> {
        let info = self.info();
        if info.tag != 1 {
            return None;
        }
        if info.length == 0 {
            return Some("");
        }
        // SAFETY: from_raw's contract covers the host-owned byte slice.
        let bytes = unsafe { core::slice::from_raw_parts(info.bytes, info.length as usize) };
        // SAFETY: the view explicitly promises UTF-8, unlike raw CodeValue
        // C strings. Revalidating a Rust host's text here defeats the borrow.
        Some(unsafe { core::str::from_utf8_unchecked(bytes) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe extern "C" fn inspect(_: *const c_void) -> CodeValueInfo {
        CodeValueInfo {
            tag: 3,
            length: 0,
            bytes: core::ptr::null(),
        }
    }
    unsafe extern "C" fn field(_: *const c_void, _: *const u8, _: u64) -> *const c_void {
        core::ptr::null()
    }
    #[test]
    fn unsupported_versions_and_missing_views_decline_before_reading_values() {
        let version = 9_u32;
        // SAFETY: only the version is read; the opaque pointer is never used.
        unsafe {
            assert!(BorrowedValue::from_raw(
                (&version as *const u32).cast(),
                core::ptr::NonNull::<u8>::dangling().as_ptr().cast()
            )
            .is_none());
            assert!(BorrowedValue::from_raw(core::ptr::null(), core::ptr::null()).is_none());
            let api = CodeBorrowedApi {
                version: 1,
                inspect,
                field,
            };
            let opaque = 0_u8;
            let view = BorrowedValue::from_raw(&api, (&opaque as *const u8).cast()).unwrap();
            assert_eq!(view.info().tag, 3);
            assert!(view.field("").is_none());
            assert!(view.text().is_none());
        }
    }
}
