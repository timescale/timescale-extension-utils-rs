
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

pub use postgres_headers_rs as pg_sys;
pub mod datum;
pub mod elog;
pub mod palloc;

//TODO postgres version
pub type FunctionCallInfoData = pg_sys::FunctionCallInfoBaseData;

// based heavily on pg-extend-rs

#[macro_export]
macro_rules! pg_fn {
    ($(pub fn $name:ident($($arg:ident : $typ:ty),* $(,)? $(; $fcinfo: ident)?) $(-> $ret:ty)? $body:block)+) => {
        $(#[no_mangle]
        pub extern "C" fn $name(fcinfo: $crate::pg_sys::FunctionCallInfo) -> $crate::pg_sys::Datum {
            // use a direct deref since this must always be set, and we can't risk a panic
            #[allow(unused_unsafe)]
            unsafe {
                $crate::palloc::in_context($crate::pg_sys::CurrentMemoryContext, || {
                    let fcinfo = &mut *fcinfo;
                    $crate::pg_fn_body!(fcinfo; $name( $($arg:$typ,)*  $(; $fcinfo)? ) $(-> $ret)? $body );
                })
            }
        })+
    };
}

#[macro_export]
macro_rules! pg_agg {
    (
        $(pub fn $name:ident($state:ident : Option<Pox<$styp:ty>> $(, $arg:ident : $typ:ty)* $(,)? $(; $fcinfo: ident)?) $(-> $ret:ty)?
            $body:block)+
    ) => {
        $(
            #[no_mangle]
            pub extern "C" fn $name(fcinfo: $crate::pg_sys::FunctionCallInfo) -> $crate::pg_sys::Datum {
                use $crate::pg_sys::{AggCheckCallContext, MemoryContext};
                // use a direct deref since this must always be set, and we can't risk a panic
                let fcinfo = unsafe { &mut *fcinfo };

                let mut agg_ctx: MemoryContext = std::ptr::null_mut();
                if unsafe {AggCheckCallContext(fcinfo, &mut agg_ctx) == 0} {
                    $crate::elog!($crate::elog::Level::Error, concat!("must call ", stringify!($name) ," as an aggregate"))
                }

                unsafe {
                    $crate::palloc::in_context(agg_ctx, || {
                        $crate::pg_fn_body!(fcinfo; $name(@$state:Option<Pox<$styp>>, $($arg:$typ,)*  $(; $fcinfo)? ) $(-> $ret)? $body );
                    })
                }
            }
        )+
    };
}

#[macro_export]
#[doc(hidden)]
macro_rules! pg_fn_body {
    ($fc:ident; $name:ident($(@$state:ident : Option<Pox<$styp:ty>>,)? $($arg:ident : $typ:ty,)* $(; $fcinfo:ident)? ) $(-> $ret:ty)? $body:block) => {
        use std::panic::{catch_unwind, AssertUnwindSafe};
        #[allow(unused_imports)]
        use $crate::{
            datum::FromOptionalDatum,
            elog::Level::Error,
            palloc::Pox,
        };
        // guard against panics in the rust code so we don't unwind into pg
        let result: Result<Option<$crate::pg_sys::Datum>, _> = catch_unwind(AssertUnwindSafe(|| {
            $(
                let $state: Option<Pox<$styp>>;
            )?
            $(
                let $arg: $typ;
            )*
            {
                #[allow(unused_variables)]
                #[allow(unused_mut)]
                let mut args = $crate::get_args(&*$fc);
                $(
                    let datum = args.next().expect("not enough arguments for aggregate state");
                    $state = <Option<*mut $styp> as FromOptionalDatum>::from_optional_datum(datum)
                        .map(|p| Pox::from_raw_unchecked(p));
                )?
                $(
                    let datum = args.next().unwrap_or_else(|| {
                        $crate::elog!(Error,
                            concat!("missing argument \"", stringify!($arg), "\""));
                        unreachable!()
                    });
                    $arg = <$typ as FromOptionalDatum>::try_from_optional_datum(datum)
                        .unwrap_or_else(|| {
                            $crate::elog!(Error,
                                concat!("NULL value for non-nullable argument \"",
                                    stringify!($arg),
                                    "\""
                                )
                            );
                            unreachable!()
                        });
                )*
            }
            $(let $fcinfo: &mut $crate::FunctionCallInfoData = $fc;)?
            #[allow(unused_variables)]
            let res = (|| { $body })();
            $(
                return <$ret as $crate::datum::ToOptionalDatum>::to_optional_datum(res);
            )?
            #[allow(unreachable_code)]
            None
        }));
        match result {
            Ok(Some(datum)) => {
                $fc.isnull = false;
                return datum;
            },
            Ok(None) => {
                $fc.isnull = true;
                return 0;
            },
            Err(err) => {
                $fc.isnull = true;
                $crate::handle_unwind(err)
            },
        }
    }
}

pub fn get_args<'a>(
    fcinfo: &'a FunctionCallInfoData
) -> impl 'a + Iterator<Item = Option<postgres_headers_rs::Datum>> {
    let num_args = fcinfo.nargs as usize;

    //TODO pg version
    return unsafe { fcinfo.args.as_slice(num_args) }
        .iter()
        .map(|nullable| {
            if nullable.isnull {
                None
            } else {
                Some(nullable.value)
            }
        });
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
        crate::elog!(#unguarded Error, "internal panic: {}", msg);
    }

    if let Some(msg) = err.downcast_ref::<String>() {
        crate::elog!(#unguarded Error, "internal panic: {}", msg);
    }

    crate::elog!(#unguarded Error, "internal panic");
    unreachable!("log should have longjmped above, this is a bug in ts-extend-rs");
}

/// marker struct that a panic is caused by a pg_error, these should be
/// converted back to postgres errors
#[must_use = "this is a marker that we must throw a postgres error"]
pub struct PGError;

impl PGError {
    pub unsafe fn re_throw(&self) -> ! {
        pg_sys::pg_re_throw();
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
    let original_exception_stack: *mut pg_sys::sigjmp_buf = pg_sys::PG_exception_stack;
    let mut local_exception_stack: mem::MaybeUninit<pg_sys::sigjmp_buf> =
        mem::MaybeUninit::uninit();
    let jumped = pg_sys::sigsetjmp(
        // grab a mutable reference, cast to a mutabl pointr, then case to the expected erased pointer type
        local_exception_stack.as_mut_ptr() as *mut pg_sys::sigjmp_buf as *mut _,
        1,
    );
    // now that we have the local_exception_stack, we set that for any PG longjmps...

    if jumped != 0 {

        pg_sys::PG_exception_stack = original_exception_stack;

        // The C Panicked!, handling control to Rust Panic handler
        compiler_fence(Ordering::SeqCst);

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

    // replace the exception stack with ours to jump to the above point
    pg_sys::PG_exception_stack = local_exception_stack.as_mut_ptr() as *mut _;

    // enforce that the setjmp is not reordered, though that's probably unlikely...
    compiler_fence(Ordering::SeqCst);
    let result = f();

    compiler_fence(Ordering::SeqCst);
    pg_sys::PG_exception_stack = original_exception_stack;

    result
}


/// try executing a closure, running `pre_re_throw` in the event that the
/// closure throws a postgres exception.
#[cfg(unix)]
#[inline(never)]
pub unsafe fn pg_try_re_throw<R, F: FnOnce() -> R, G: FnOnce()>(
    f: F, pre_re_throw: G
) -> R {
    // setup the check protection
    let original_exception_stack: *mut pg_sys::sigjmp_buf = pg_sys::PG_exception_stack;
    let mut local_exception_stack: mem::MaybeUninit<pg_sys::sigjmp_buf> =
        mem::MaybeUninit::uninit();
    let jumped = pg_sys::sigsetjmp(
        // grab a mutable reference, cast to a mutabl pointr, then case to the expected erased pointer type
        local_exception_stack.as_mut_ptr() as *mut pg_sys::sigjmp_buf as *mut _,
        1,
    );
    // now that we have the local_exception_stack, we set that for any PG longjmps...

    if jumped != 0 {
        pg_sys::PG_exception_stack = original_exception_stack;

        // The C Panicked!, handling control to Rust Panic handler
        compiler_fence(Ordering::SeqCst);
        pre_re_throw();
        pg_sys::pg_re_throw();
    }

    // replace the exception stack with ours to jump to the above point
    pg_sys::PG_exception_stack = local_exception_stack.as_mut_ptr() as *mut _;

    // enforce that the setjmp is not reordered, though that's probably unlikely...
    compiler_fence(Ordering::SeqCst);
    let result = f();

    compiler_fence(Ordering::SeqCst);
    pg_sys::PG_exception_stack = original_exception_stack;

    result
}

#[cfg(test)]
mod tests {

    crate::pg_fn!{
        pub fn compile_test(a: i32) -> i32 {
            return a + 1
        }
    }

    crate::pg_fn!{
        pub fn compile_test_noarg() -> i32 {
            return 1
        }
    }

    crate::pg_fn!{
        pub fn compile_test_noret() {
            return
        }
    }

    crate::pg_fn!{
        pub fn compile_test_optional(a: i32, b: Option<i32>) -> i32 {
            match b {
                Some(b) => a + b,
                None => a,
            }
        }
    }

    crate::pg_fn!{
        pub fn compile_test_fcinfo(; fcinfo) -> i16 {
            fcinfo.nargs
        }
    }

    crate::pg_fn!{
        pub fn compile_test_multi0(a: u32, b: u32; fcinfo) -> u32 {
            a + b + fcinfo.nargs as u32
        }

        pub fn compile_test_multi1() -> u64 {
            0
        }
    }

    crate::pg_agg!{
        pub fn compile_test_sfunc(state: Option<Pox<usize>>) -> Option<Pox<usize>> {
            state
        }

        pub fn compile_test_final(state: Option<Pox<usize>>) -> usize {
            state.map(|s| *s).unwrap_or_else(|| 0)
        }
    }
}
