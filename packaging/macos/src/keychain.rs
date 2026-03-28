//! Keychain credential access — ADR-024 Phase H PH-02.
//!
//! Reads VEX_MODEL_TOKEN from the macOS system keychain using the
//! Security.framework `SecItemCopyMatching` API directly via FFI.
//! No external crates are used; both Security and CoreFoundation are
//! linked through the standard macOS SDK paths with `#[link(name = ...,
//! kind = "framework")]`.
//!
//! Operators store the credential once with the system `security` CLI:
//!
//!   security add-generic-password \
//!     -s vexcoder \
//!     -a VEX_MODEL_TOKEN \
//!     -w <token>
//!
//! The launcher injects the retrieved value as `VEX_MODEL_TOKEN` into the
//! vex process environment at startup.  The token is never written to disk
//! by this launcher.

/// Service name registered in the keychain.
const SERVICE: &str = "vexcoder";
/// Account name used to identify the model-endpoint token item.
const ACCOUNT: &str = "VEX_MODEL_TOKEN";

/// Reads `VEX_MODEL_TOKEN` from the macOS keychain.
///
/// Returns `None` when:
/// - no matching item exists in the keychain,
/// - the item data is empty or not valid UTF-8, or
/// - the function is called on a non-macOS platform.
pub fn read_model_token() -> Option<String> {
    read_generic_password(SERVICE, ACCOUNT)
}

#[cfg(target_os = "macos")]
fn read_generic_password(service: &str, account: &str) -> Option<String> {
    use macos_ffi::*;
    use std::ffi::CString;

    let service_cs = CString::new(service).ok()?;
    let account_cs = CString::new(account).ok()?;

    // SAFETY: All CF objects created here are released before the function
    // returns.  The `SecItemCopyMatching` result is a CFDataRef whose
    // lifetime is tied to the caller; we copy the bytes before releasing it.
    unsafe {
        let svc_str = CFStringCreateWithCString(
            std::ptr::null(),
            service_cs.as_ptr(),
            kCFStringEncodingUTF8,
        );
        let acct_str = CFStringCreateWithCString(
            std::ptr::null(),
            account_cs.as_ptr(),
            kCFStringEncodingUTF8,
        );

        if svc_str.is_null() || acct_str.is_null() {
            if !svc_str.is_null() {
                CFRelease(svc_str as CFTypeRef);
            }
            if !acct_str.is_null() {
                CFRelease(acct_str as CFTypeRef);
            }
            return None;
        }

        let keys: [CFTypeRef; 5] = [
            kSecClass as CFTypeRef,
            kSecAttrService as CFTypeRef,
            kSecAttrAccount as CFTypeRef,
            kSecReturnData as CFTypeRef,
            kSecMatchLimit as CFTypeRef,
        ];
        let values: [CFTypeRef; 5] = [
            kSecClassGenericPassword as CFTypeRef,
            svc_str as CFTypeRef,
            acct_str as CFTypeRef,
            kCFBooleanTrue,
            kSecMatchLimitOne as CFTypeRef,
        ];

        let query = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            5,
            &raw const kCFTypeDictionaryKeyCallBacks as *const _,
            &raw const kCFTypeDictionaryValueCallBacks as *const _,
        );

        let mut result: CFTypeRef = std::ptr::null();
        let status = SecItemCopyMatching(query, &raw mut result);

        CFRelease(query as CFTypeRef);
        CFRelease(svc_str as CFTypeRef);
        CFRelease(acct_str as CFTypeRef);

        if status != errSecSuccess || result.is_null() {
            return None;
        }

        // The result is a CFDataRef containing the raw password bytes.
        let data = result as CFDataRef;
        let len = CFDataGetLength(data) as usize;
        let ptr = CFDataGetBytePtr(data);
        let bytes = std::slice::from_raw_parts(ptr, len);
        let token = std::str::from_utf8(bytes)
            .ok()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned);

        CFRelease(result);
        token
    }
}

#[cfg(not(target_os = "macos"))]
fn read_generic_password(_service: &str, _account: &str) -> Option<String> {
    None
}

/// Raw FFI bindings to Security.framework and CoreFoundation.
///
/// Only the symbols required for `SecItemCopyMatching` with a
/// `kSecClassGenericPassword` query are declared.
#[cfg(target_os = "macos")]
mod macos_ffi {
    use std::ffi::{c_char, c_void};

    // ---- CoreFoundation opaque types ----------------------------------------

    /// Opaque pointer representing any CoreFoundation object.
    pub type CFTypeRef = *const c_void;
    /// Opaque pointer to a CFString object.
    pub type CFStringRef = *const c_void;
    /// Opaque pointer to a CFData object.
    pub type CFDataRef = *const c_void;
    /// Opaque pointer to a CFDictionary object.
    pub type CFDictionaryRef = *const c_void;
    /// Index type used by CoreFoundation collection APIs.
    pub type CFIndex = isize;
    /// String encoding identifier.
    pub type CFStringEncoding = u32;

    /// UTF-8 string encoding constant.
    pub const kCFStringEncodingUTF8: CFStringEncoding = 0x0800_0100;

    // ---- Security.framework types -------------------------------------------

    /// Status code returned by Security APIs.
    pub type OSStatus = i32;
    /// No error.
    pub const errSecSuccess: OSStatus = 0;

    // ---- Framework link declarations ----------------------------------------

    #[link(name = "Security", kind = "framework")]
    extern "C" {
        /// Keychain item class for generic passwords.
        pub static kSecClass: CFStringRef;
        pub static kSecClassGenericPassword: CFStringRef;
        /// Keychain attribute: service name.
        pub static kSecAttrService: CFStringRef;
        /// Keychain attribute: account name.
        pub static kSecAttrAccount: CFStringRef;
        /// Query key: return item data as CFData.
        pub static kSecReturnData: CFStringRef;
        /// Query key: maximum number of results to return.
        pub static kSecMatchLimit: CFStringRef;
        /// Value for kSecMatchLimit: return at most one item.
        pub static kSecMatchLimitOne: CFStringRef;

        /// Copies matching keychain items into `result`.
        pub fn SecItemCopyMatching(
            query: CFDictionaryRef,
            result: *mut CFTypeRef,
        ) -> OSStatus;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        /// Canonical `true` value used in CF dictionary queries.
        pub static kCFBooleanTrue: CFTypeRef;

        /// Callback tables for CF dictionary key and value ownership.
        pub static kCFTypeDictionaryKeyCallBacks: c_void;
        pub static kCFTypeDictionaryValueCallBacks: c_void;

        /// Creates a CFString from a NUL-terminated C string.
        pub fn CFStringCreateWithCString(
            alloc: CFTypeRef,
            c_str: *const c_char,
            encoding: CFStringEncoding,
        ) -> CFStringRef;

        /// Creates a CFDictionary from parallel key and value arrays.
        pub fn CFDictionaryCreate(
            alloc: CFTypeRef,
            keys: *const CFTypeRef,
            values: *const CFTypeRef,
            num_values: CFIndex,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> CFDictionaryRef;

        /// Returns the number of bytes in a CFData object.
        pub fn CFDataGetLength(the_data: CFDataRef) -> CFIndex;

        /// Returns a read-only pointer to the byte buffer of a CFData object.
        pub fn CFDataGetBytePtr(the_data: CFDataRef) -> *const u8;

        /// Releases a CoreFoundation object.
        pub fn CFRelease(cf: CFTypeRef);
    }
}

#[cfg(test)]
mod tests {
    use super::read_model_token;

    /// Verifies the function does not panic when no keychain item is present,
    /// which is the expected state on any CI runner without a provisioned item.
    #[test]
    fn read_model_token_is_non_panicking_when_absent() {
        let _ = read_model_token();
    }

    /// Verifies that the function returns `None` rather than an empty string
    /// when there is no token to return.
    #[test]
    fn read_model_token_does_not_return_empty_string() {
        if let Some(token) = read_model_token() {
            assert!(!token.is_empty(), "token must not be an empty string");
        }
    }
}
