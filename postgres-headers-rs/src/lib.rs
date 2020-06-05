#![allow(nonstandard_style)]
#![allow(improper_ctypes)]

// TODO version selection
// TODO should we have a sperate linux_musl_pgN target?
#[cfg(target_os = "linux")]
pub use linux_glibc_pg12::*;

#[cfg(target_os = "macos")]
pub use macos_pg12::*;

#[cfg(target_os = "linux")]
mod linux_glibc_pg12;

#[cfg(target_os = "macos")]
mod macos_pg12;
