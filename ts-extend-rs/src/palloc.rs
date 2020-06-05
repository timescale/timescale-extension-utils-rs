
use std::alloc::{GlobalAlloc, Layout};

use crate::pg_sys::{
    CurrentMemoryContext,
    MemoryContext,
    MemoryContextAlloc,
    MemoryContextAllocZero,
    pfree,
    repalloc
};

/// run code in a given memory context, switching back to the original context
/// on exit, through either return or panic
pub unsafe fn in_context<T, F>(context: MemoryContext, f: F) -> T
where F: FnOnce() -> T {
    // we need a variable her so the guard lives to the end of this scope
    let _guard = MemoryContextGuard(context);
    f()
}

/// this struct will swap the current memory context to the one it contains
/// when it is dropped. it is recommended that `in_context` is used intead of
/// using this directly
pub struct MemoryContextGuard(pub MemoryContext);
impl Drop for MemoryContextGuard {
    fn drop(&mut self) {
        unsafe {
            memory_context_switch_to(self.0);
        }
    }
}

/// switch `CurrentMemoryContext` to a given context, returning the old memory
/// contect. It is recommended that `in_context()` be used instead, as that
/// function handles switching the memory context back on panic.
pub unsafe fn memory_context_switch_to(context: MemoryContext) -> MemoryContext {
    let old = CurrentMemoryContext;
    CurrentMemoryContext = context;
    old
}

#[global_allocator]
static GLOBAL: PallocAllocator = PallocAllocator;

struct PallocAllocator;

/// There is an uncomfortable mismatch between rust's memory allocation, and
/// postgres's; rust tries to clean memory by using stack-based destructors,
/// while postgres does so using arenas. The issue we encounter is that postgres
/// implements exception-handling using setjmp/longjmp, which will can jump over
/// stack frames containing rust destructors. To avoid needing to register a
/// setjmp handler at every call to a postgres function, we want to use
/// postgres's MemoryContexts to manage memory, even though this is not strictly
/// speaking safe. As a compromise, it may be better to use the TransactionContext
/// by default, as it is relatively long-lived, and will clean up on errors.
unsafe impl GlobalAlloc for PallocAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        MemoryContextAlloc(CurrentMemoryContext, layout.size() as _)  as *mut _
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        pfree(ptr as *mut _)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        MemoryContextAllocZero(CurrentMemoryContext, layout.size() as _)  as *mut _
    }

    unsafe fn realloc(&self, ptr: *mut u8, _layout: Layout, new_size: usize) -> *mut u8 {
        repalloc(ptr as *mut _, new_size as _) as *mut _
    }
}
