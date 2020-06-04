#![allow(nonstandard_style)]
#![allow(improper_ctypes)]

//TODO version selection
#[cfg(target_os = "macos")]
pub use macos_pg12::*;

#[cfg(target_os = "macos")]
mod macos_pg12;
