// SPDX-FileCopyrightText: 2026 BigLinux contributors
// SPDX-License-Identifier: MIT

//! jemalloc as the allocator of the whole process, linked in at build time.
//!
//! Defining the C allocation functions here makes every allocation in the
//! executable (Rust, GTK, GLib, libmpv, and libraries opened later with
//! `dlopen`) go through the statically linked jemalloc (prefixed `_rjem_`, so
//! its own symbols never collide with libc). Unlike `LD_PRELOAD`, nothing
//! reaches child processes such as ffmpeg: they start on glibc malloc.
//!
//! `free_sized` and `free_aligned_sized` are not defined here: jemalloc
//! 5.3.1 (bundled by `tikv-jemalloc-sys`) crashes on `free_sized(NULL, n)`,
//! which GLib ≥ 2.86 issues through `g_free_sized`; glibc's versions of both
//! simply jump to `free`, which lands here and handles NULL.
//!
//! The executable must export these symbols (`--export-dynamic-symbol`, see the
//! players' `build.rs`) so libraries loaded with `dlopen` bind to them too.

use std::ffi::{c_int, c_void};

use tikv_jemalloc_sys as jemalloc;

/// Built-in jemalloc tuning: one
/// arena, freed pages returned to the OS at once, large buffers kept out of
/// the arena. jemalloc reads it before `main`; `_RJEM_MALLOC_CONF` still
/// overrides it at run time.
#[unsafe(export_name = "_rjem_malloc_conf")]
static MALLOC_CONF: &u8 =
    &b"narenas:1,dirty_decay_ms:0,muzzy_decay_ms:0,oversize_threshold:131072,retain:false\0"[0];

fn page_size() -> usize {
    // SAFETY: sysconf only reads a constant and never allocates.
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    usize::try_from(size).unwrap_or(4096)
}

#[unsafe(no_mangle)]
unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    // SAFETY: same contract as C malloc.
    unsafe { jemalloc::malloc(size) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn calloc(count: usize, size: usize) -> *mut c_void {
    // SAFETY: same contract as C calloc.
    unsafe { jemalloc::calloc(count, size) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    // SAFETY: `ptr` is NULL or came from this allocator (every allocation does).
    unsafe { jemalloc::realloc(ptr, size) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn free(ptr: *mut c_void) {
    // SAFETY: `ptr` is NULL or came from this allocator; jemalloc ignores NULL.
    unsafe { jemalloc::free(ptr) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn posix_memalign(out: *mut *mut c_void, align: usize, size: usize) -> c_int {
    // SAFETY: same contract as C posix_memalign.
    unsafe { jemalloc::posix_memalign(out, align, size) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn aligned_alloc(align: usize, size: usize) -> *mut c_void {
    // SAFETY: same contract as C aligned_alloc.
    unsafe { jemalloc::aligned_alloc(align, size) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn memalign(align: usize, size: usize) -> *mut c_void {
    // SAFETY: same contract as C aligned_alloc, which memalign matches.
    unsafe { jemalloc::aligned_alloc(align, size) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn valloc(size: usize) -> *mut c_void {
    // SAFETY: the page size is a valid power-of-two alignment.
    unsafe { jemalloc::aligned_alloc(page_size(), size) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn pvalloc(size: usize) -> *mut c_void {
    let page = page_size();
    let Some(rounded) = size.max(1).checked_next_multiple_of(page) else {
        return std::ptr::null_mut();
    };
    // SAFETY: the page size is a valid power-of-two alignment.
    unsafe { jemalloc::aligned_alloc(page, rounded) }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn malloc_usable_size(ptr: *const c_void) -> usize {
    // SAFETY: `ptr` is NULL or came from this allocator; jemalloc returns 0 for NULL.
    unsafe { jemalloc::malloc_usable_size(ptr) }
}
