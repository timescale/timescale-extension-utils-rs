
use std::{
    alloc::{GlobalAlloc, Layout},
    ptr::{NonNull, null_mut},
    marker::PhantomData,
};

use crate::pg_sys::{
    CurrentMemoryContext,
    MemoryContext,
    MemoryContextAlloc,
    MemoryContextAllocZero,
    pfree,
    repalloc
};

/// `Pox` offers the same API as `Box`, except that it is not freed on `drop`,
/// making it safe to use for e.g. the first argument of an aggregate.
pub struct Pox<T: ?Sized>(NonNull<T>, PhantomData<T>);

impl<T> Pox<T> {
    pub fn new(val: T) -> Self {
        unsafe {
            Pox(NonNull::new_unchecked(Box::into_raw(Box::new(val))), PhantomData)
        }
    }

    pub unsafe fn from_raw(ptr: *mut T) -> Option<Self> {
        NonNull::new(ptr).map(|n| Pox(n, PhantomData))
    }

    pub unsafe fn from_raw_unchecked(ptr: *mut T) -> Self {
        Pox(NonNull::new_unchecked(ptr), PhantomData)
    }

    pub fn into_raw(self) -> *mut T {
        self.0.as_ptr()
    }
}

impl<T: ?Sized> From<Box<T>> for Pox<T> {
    fn from(val: Box<T>) -> Self {
        unsafe {
            Pox(NonNull::new_unchecked(Box::into_raw(val)), PhantomData)
        }
    }
}

impl<T> From<T> for Pox<T> {
    fn from(val: T) -> Self {
        Pox::new(val)
    }
}

impl<T: ?Sized> std::ops::Deref for Pox<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { self.0.as_ref() }
    }
}

impl<T: ?Sized> std::ops::DerefMut for Pox<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.0.as_mut() }
    }
}


/// run code in a given memory context, switching back to the original context
/// on exit, through either return or panic
pub unsafe fn in_context<T, F>(context: MemoryContext, f: F) -> T
where F: FnOnce() -> T {
    // we need a variable her so the guard lives to the end of this scope
    let _guard = MemoryContextGuard(GLOBAL.0);
    GLOBAL.0 = context;
    // crate::pg_try_re_throw(f, || GLOBAL.0 = guard.0)
    f()
}

/// this struct will swap the current memory context to the one it contains
/// when it is dropped. it is recommended that `in_context` is used intead of
/// using this directly
pub struct MemoryContextGuard(pub MemoryContext);
impl Drop for MemoryContextGuard {
    fn drop(&mut self) {
        unsafe {
            GLOBAL.0 = self.0;
        }
    }
}

/// switch `CurrentMemoryContext` to a given context, returning the old memory
/// contect. It is recommended that `in_context()` be used instead, as that
/// function will switch only the rust MemoryContext, and handles switching the
/// memory context back on panic.
pub unsafe fn memory_context_switch_to(context: MemoryContext) -> MemoryContext {
    let old = CurrentMemoryContext;
    CurrentMemoryContext = context;
    old
}

#[global_allocator]
static mut GLOBAL: PallocAllocator = PallocAllocator(null_mut());

struct PallocAllocator(MemoryContext);

extern "C" {
    pub static mut TopMemoryContext: MemoryContext;
    pub static mut TopTransactionContext: MemoryContext;
}

/// There is an uncomfortable mismatch between rust's memory allocation and
/// postgres's; rust tries to clean memory by using stack-based destructors,
/// while postgres does so using arenas. The issue we encounter is that postgres
/// implements exception-handling using setjmp/longjmp, which will can jump over
/// stack frames containing rust destructors. To avoid needing to register a
/// setjmp handler at every call to a postgres function, we want to use
/// postgres's MemoryContexts to manage memory, even though this is not strictly
/// speaking safe. As a compromise, bny default use the TransactionContext to
/// allocate all memory; it is fairly rare we want data to live across
/// transactions, so it should be fairly rare we get memory freed out from under
/// us, but the memory will be freed if the transaction aborts.
unsafe impl GlobalAlloc for PallocAllocator {
    //FIXME allow for switching the memory context allocated in
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut mctx = TopTransactionContext;
        if GLOBAL.0 != null_mut() {
            mctx = GLOBAL.0;
        }
        MemoryContextAlloc(mctx, layout.size() as _)  as *mut _
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        pfree(ptr as *mut _)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let mut mctx = TopTransactionContext;
        if GLOBAL.0 != null_mut() {
            mctx = GLOBAL.0;
        }
        MemoryContextAllocZero(mctx, layout.size() as _)  as *mut _
    }

    unsafe fn realloc(&self, ptr: *mut u8, _layout: Layout, new_size: usize) -> *mut u8 {
        repalloc(ptr as *mut _, new_size as _) as *mut _
    }
}
