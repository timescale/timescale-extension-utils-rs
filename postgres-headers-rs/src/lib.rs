#![allow(nonstandard_style)]
#![allow(improper_ctypes)]

#[cfg(not(feature = "parse_headers"))]
pub use cached::*;

#[cfg(feature = "parse_headers")]
pub use generated::*;

#[cfg(not(feature = "parse_headers"))]
mod cached;

#[cfg(feature = "parse_headers")]
mod generated {
    #[cfg(target_os = "linux")]
    use std::os::raw::c_int;

    include!(concat!(env!("OUT_DIR"), "/generated.rs"));

    #[cfg(target_os = "linux")]
    extern "C" {
        #[link_name = "__sigsetjmp"]
        pub fn sigsetjmp(env: *mut sigjmp_buf, savemask: c_int) -> c_int;
    }
}
