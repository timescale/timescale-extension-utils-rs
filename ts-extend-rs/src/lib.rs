
// use std::{
//     mem,
//     os::raw::c_int,
//     sync::atomic::{
//         compiler_fence,
//         Ordering,
//     },
// };

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
            let fcinfo = unsafe { &mut *fcinfo };
            $crate::pg_fn_body!(fcinfo; $name( $($arg:$typ,)*  $(; $fcinfo)? ) $(-> $ret)? $body );
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
                    let datum = args.next().expect("not enough arguments for function");
                    $arg = <$typ as FromOptionalDatum>::from_optional_datum(datum);
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
                use std::sync::atomic::{
                    compiler_fence,
                    Ordering,
                };
                $fc.isnull = true;

                // setup to jump back to postgres code
                compiler_fence(Ordering::SeqCst);
                if let Some(msg) = err.downcast_ref::<&'static str>() {
                    $crate::elog!(Error, "panic executing Rust '{}': {}", stringify!($name), msg);
                }

                if let Some(msg) = err.downcast_ref::<String>() {
                    $crate::elog!(Error, "panic executing Rust '{}': {}", stringify!($name), msg);
                }

                $crate::elog!(Error, "panic executing Rust '{}'", stringify!(#func_name));
                unreachable!("log should have longjmped above, this is a bug in ts-extend-rs");
            },
        }
    }
}

#[macro_export]
#[doc(hidden)]
macro_rules! setup_fn_args {
    ($fc:ident; $($arg:ident : $typ:ty),* $(,)? $(; $fcinfo: ident)?) => {
        $(
            let $arg: $typ;
        )*
        {
            #[allow(unused_variables)]
            #[allow(unused_mut)]
            let mut args = $crate::get_args(&*$fc);
            $(
                let datum = args.next().expect("not enough arguments for function");
                $arg = <$typ as $crate::datum::FromOptionalDatum>::from_optional_datum(datum);
            )*
        }
        $(let $fcinfo: &mut $crate::FunctionCallInfoData = $fc;)?
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

// /// Information for a longjmp
// struct JumpContext {
//     jump_value: c_int,
// }

// unsafe fn pg_sys_longjmp(_buf: *mut c_int, _value: ::std::os::raw::c_int) {
//     pg_sys::siglongjmp(_buf, _value);
// }

// /// Provides a barrier between Rust and Postgres' usage of the C set/longjmp
// ///
// /// In the case of a longjmp being caught, this will convert that to a panic. For this to work
// ///   properly, there must be a Rust panic handler (see crate::register_panic_handler).PanicContext
// ///   If the `pg_exern` attribute macro is used for exposing Rust functions to Postgres, then
// ///   this is already handled.
// ///
// /// See the man pages for info on setjmp http://man7.org/linux/man-pages/man3/setjmp.3.html
// #[cfg(unix)]
// #[inline(never)]
// pub(crate) unsafe fn guard_pg<R, F: FnOnce() -> R>(f: F) -> R {
//     // setup the check protection
//     let original_exception_stack: *mut pg_sys::sigjmp_buf = pg_sys::PG_exception_stack;
//     let mut local_exception_stack: mem::MaybeUninit<pg_sys::sigjmp_buf> =
//         mem::MaybeUninit::uninit();
//     let jumped = pg_sys::sigsetjmp(
//         // grab a mutable reference, cast to a mutabl pointr, then case to the expected erased pointer type
//         local_exception_stack.as_mut_ptr() as *mut pg_sys::sigjmp_buf as *mut _,
//         1,
//     );
//     // now that we have the local_exception_stack, we set that for any PG longjmps...

//     if jumped != 0 {
//         pg_sys::PG_exception_stack = original_exception_stack;

//         // The C Panicked!, handling control to Rust Panic handler
//         compiler_fence(Ordering::SeqCst);
//         panic!(JumpContext { jump_value: jumped });
//     }

//     // replace the exception stack with ours to jump to the above point
//     pg_sys::PG_exception_stack = local_exception_stack.as_mut_ptr() as *mut _;

//     // enforce that the setjmp is not reordered, though that's probably unlikely...
//     compiler_fence(Ordering::SeqCst);
//     let result = f();

//     compiler_fence(Ordering::SeqCst);
//     pg_sys::PG_exception_stack = original_exception_stack;

//     result
// }

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
