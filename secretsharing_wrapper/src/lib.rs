use std::os::raw::c_char;
use std::ffi::{CString, CStr};

extern "C" {
    pub  fn disassemble_secret(secret_data: *const c_char) -> *mut c_char;
    pub  fn reassemble_secret(serialized_data: *const c_char) -> *mut c_char;
}

pub async fn disassemble(secret: &str) -> Result<String, String> {
    let c_secret = CString::new(secret).expect("CString::new failed");
    unsafe {
        let result = disassemble_secret(c_secret.as_ptr());
        if result.is_null() {
            Err("Null pointer received from disassemble_secret".to_string())
        } else {
            let c_str = CStr::from_ptr(result);
            let r_str = c_str.to_str().expect("Failed to convert CStr to str");
            Ok(r_str.to_owned())
        }
    }
}

pub async fn reassemble(data: &str) -> Result<String, String> {
    let c_data = CString::new(data).expect("CString::new failed");
    unsafe {
        let result = reassemble_secret(c_data.as_ptr());
        if result.is_null() {
            Err("Null pointer received from reassemble_secret".to_string())
        } else {
            let c_str = CStr::from_ptr(result);
            let r_str = c_str.to_str().expect("Failed to convert CStr to str");
            Ok(r_str.to_owned())
        }
    }
}