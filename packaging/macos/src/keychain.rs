const SERVICE: &str = "vexcoder";

const ACCOUNT: &str = "VEX_MODEL_TOKEN";

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

        let data = result as CFDataRef;
        let len = CFDataGetLength(data) as usize;
        let ptr = CFDataGetBytePtr(data);
        let bytes: &[u8] = if len == 0 {
            &[]
        } else {
            if ptr.is_null() {
                CFRelease(result);
                return None;
            }
            std::slice::from_raw_parts(ptr, len)
        };
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

#[cfg(target_os = "macos")]
mod macos_ffi {
    use std::ffi::{c_char, c_void};

    pub type CFTypeRef = *const c_void;
    
    pub type CFStringRef = *const c_void;
    
    pub type CFDataRef = *const c_void;
    
    pub type CFDictionaryRef = *const c_void;
    
    pub type CFIndex = isize;
    
    pub type CFStringEncoding = u32;

    pub const kCFStringEncodingUTF8: CFStringEncoding = 0x0800_0100;

    pub type OSStatus = i32;
    
    pub const errSecSuccess: OSStatus = 0;

    #[link(name = "Security", kind = "framework")]
    extern "C" {
        
        pub static kSecClass: CFStringRef;
        pub static kSecClassGenericPassword: CFStringRef;
        
        pub static kSecAttrService: CFStringRef;
        
        pub static kSecAttrAccount: CFStringRef;
        
        pub static kSecReturnData: CFStringRef;
        
        pub static kSecMatchLimit: CFStringRef;
        
        pub static kSecMatchLimitOne: CFStringRef;

        pub fn SecItemCopyMatching(
            query: CFDictionaryRef,
            result: *mut CFTypeRef,
        ) -> OSStatus;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        
        pub static kCFBooleanTrue: CFTypeRef;

        pub static kCFTypeDictionaryKeyCallBacks: c_void;
        pub static kCFTypeDictionaryValueCallBacks: c_void;

        pub fn CFStringCreateWithCString(
            alloc: CFTypeRef,
            c_str: *const c_char,
            encoding: CFStringEncoding,
        ) -> CFStringRef;

        pub fn CFDictionaryCreate(
            alloc: CFTypeRef,
            keys: *const CFTypeRef,
            values: *const CFTypeRef,
            num_values: CFIndex,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> CFDictionaryRef;

        pub fn CFDataGetLength(the_data: CFDataRef) -> CFIndex;

        pub fn CFDataGetBytePtr(the_data: CFDataRef) -> *const u8;

        pub fn CFRelease(cf: CFTypeRef);
    }
}

#[cfg(test)]
mod tests {
    use super::read_model_token;

    #[test]
    fn read_model_token_is_non_panicking_when_absent() {
        let _ = read_model_token();
    }

    #[test]
    fn read_model_token_does_not_return_empty_string() {
        if let Some(token) = read_model_token() {
            assert!(!token.is_empty(), "token must not be an empty string");
        }
    }
}
