#![allow(nonstandard_style)]
#![allow(improper_ctypes)]

#[cfg(feature = "parse_headers")]
extern crate pg_guard_function;

use std::{
    any::Any,
    mem,
    panic,
    sync::{
        atomic::{
            compiler_fence,
            Ordering,
        },
        Once,
    },
};

pub mod sys {
    #[cfg(not(feature = "parse_headers"))]
    pub use crate::cached::*;

    #[cfg(feature = "parse_headers")]
    pub use crate::generated::*;
}

#[cfg(not(feature = "parse_headers"))]
mod cached;

#[cfg(feature = "parse_headers")]
mod generated {
    pub use self::bindgenerated::*;

    include!(concat!(env!("OUT_DIR"), "/generated.rs"));
}

pub mod elog;

#[cfg(all(target_os = "linux", target_env = "gnu"))]
extern "C" {
    #[link_name = "__sigsetjmp"]
    pub fn sigsetjmp(env: *mut sigjmp_buf, savemask: c_int) -> c_int;
}

/// handle an error returned by `catch_unwind` in the proper manner; errors
/// generated by rust will be turned into `elog!(Error, ...)` while ones created
/// by postgres itself will be re-thrown
pub fn handle_unwind(err: Box<dyn Any + Send + 'static>) -> ! {
    use crate::elog::Level::Error;

    // setup to jump back to postgres code
    compiler_fence(Ordering::SeqCst);
    if let Some(err) = err.downcast_ref::<PGError>() {
        unsafe {
            err.re_throw()
        }
    }

    if let Some(msg) = err.downcast_ref::<&'static str>() {
        crate::elog!(Error, "internal panic: {}", msg);
    }

    if let Some(msg) = err.downcast_ref::<String>() {
        crate::elog!(Error, "internal panic: {}", msg);
    }

    crate::elog!(Error, "internal panic");
    unreachable!("log should have longjmped above, this is a bug in ts-extend-rs");
}

/// marker struct that a panic is caused by a pg_error, these should be
/// converted back to postgres errors
#[must_use = "this is a marker that we must throw a postgres error"]
pub struct PGError;

impl PGError {
    pub unsafe fn re_throw(&self) -> ! {
        crate::sys::pg_re_throw();
        // this should not be reachable due to the above rethrow
        std::process::abort()
    }
}

/// Provides a barrier between Rust and Postgres' usage of the C set/longjmp
///
/// In the case of a longjmp being caught, this will convert that to a panic.
/// The panic must be caught _before_ unwinding into C code.
#[cfg(unix)]
#[inline(never)]
pub unsafe fn guard_pg<R, F: FnOnce() -> R>(f: F) -> R {
    // setup the check protection
    let original_exception_stack: *mut crate::sys::sigjmp_buf = crate::sys::PG_exception_stack;
    let mut local_exception_stack: mem::MaybeUninit<crate::sys::sigjmp_buf> =
        mem::MaybeUninit::uninit();
    let jumped = crate::sys::sigsetjmp(
        // grab a mutable reference, cast to a mutabl pointr, then case to the expected erased pointer type
        local_exception_stack.as_mut_ptr() as *mut crate::sys::sigjmp_buf as *mut _,
        1,
    );
    // now that we have the local_exception_stack, we set that for any PG longjmps...

    if jumped != 0 {
        crate::sys::PG_exception_stack = original_exception_stack;

        // The C Panicked!, handling control to Rust Panic handler
        compiler_fence(Ordering::SeqCst);
        handle_pg_unwind()
    }

    // replace the exception stack with ours to jump to the above point
    crate::sys::PG_exception_stack = local_exception_stack.as_mut_ptr() as *mut _;

    // enforce that the setjmp is not reordered, though that's probably unlikely...
    compiler_fence(Ordering::SeqCst);
    let result = f();

    compiler_fence(Ordering::SeqCst);
    crate::sys::PG_exception_stack = original_exception_stack;

    result
}

#[cfg(unix)]
#[inline(never)]
#[cold]
fn handle_pg_unwind() -> ! {
    // if this is the first time we've caught a postgres error, set a panic
    // hook so rust does not print an additional panic for the error.
    static SET_PANIC_HOOK: Once = Once::new();
    SET_PANIC_HOOK.call_once(|| {
        let default_handler = panic::take_hook();
        let our_handler: Box<dyn Fn(&panic::PanicInfo<'_>) + Sync + Send> =
            Box::new(move |info| {
                match info.payload().downcast_ref::<PGError>() {
                    // for pg errors postgres will handle the output
                    Some(_) => {},
                    // other errors we still output
                    None => default_handler(info),
                }
            });
        panic::set_hook(our_handler);
    });
    panic!(PGError);
}
