
#[cfg(target_os = "linux")]
use std::os::raw::c_int;

// TODO version selection
// TODO should we have a separate linux_musl_pgN target?
#[cfg(target_os = "linux")]
pub use linux_glibc_pg12::*;

#[cfg(target_os = "macos")]
pub use macos_pg12::*;

#[cfg(target_os = "linux")]
mod linux_glibc_pg12;

#[cfg(target_os = "macos")]
mod macos_pg12;

#[cfg(target_os = "linux")]
extern "C" {
    #[link_name = "__sigsetjmp"]
    pub fn sigsetjmp(env: *mut sigjmp_buf, savemask: c_int) -> c_int;
}
