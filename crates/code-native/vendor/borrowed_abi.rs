//! Optional metadata-dispatch wire types; mirrored in code-native/vendor.
use core::ffi::c_void;

#[repr(C)]
pub struct CodeValueInfo {
    /// The existing CodeTag integer. Scalars other than strings expose only tag.
    pub tag: i32,
    /// Array/object element count or UTF-8 byte count; zero for other scalars.
    pub length: u64,
    /// Borrowed UTF-8 bytes for strings, null otherwise (also allowed if empty).
    pub bytes: *const u8,
}

#[repr(C)]
pub struct CodeBorrowedApi {
    pub version: u32,
    pub inspect: unsafe extern "C" fn(*const c_void) -> CodeValueInfo,
    /// First matching field; null for missing/non-object. Key is UTF-8, not CStr.
    pub field: unsafe extern "C" fn(*const c_void, *const u8, u64) -> *const c_void,
}
