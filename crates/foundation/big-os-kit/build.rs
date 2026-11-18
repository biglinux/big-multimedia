// SPDX-License-Identifier: MIT
//! Publishes the C allocation functions `src/allocator.rs` defines, so an
//! executable's build script reads them as `DEP_BIG_OS_KIT_ALLOCATOR_SYMBOLS`
//! and exports each one.

fn main() {
    // allow-noisy-log: cargo build directives, not log output.
    println!("cargo::rerun-if-changed=build.rs");
    println!(
        "cargo::metadata=allocator_symbols=malloc,calloc,realloc,free,posix_memalign,\
         aligned_alloc,memalign,valloc,pvalloc,malloc_usable_size"
    );
}
