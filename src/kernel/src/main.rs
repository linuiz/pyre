// TODO package details
#![no_std]
#![no_main]
#![feature(
    error_in_core,                          // #103765 <https://github.com/rust-lang/rust/issues/103765>
    result_flattening,                      // #70142 <https://github.com/rust-lang/rust/issues/70142>
    duration_constants,                     // #57391 <https://github.com/rust-lang/rust/issues/57391>
    iter_advance_by,                        // #77404 <https://github.com/rust-lang/rust/issues/77404>
    iter_array_chunks,                      // #100450 <https://github.com/rust-lang/rust/issues/100450>
    iter_next_chunk,                        // #98326 <https://github.com/rust-lang/rust/issues/98326>
    array_windows,                          // #75027 <https://github.com/rust-lang/rust/issues/75027>
    maybe_uninit_slice,                     // #63569 <https://github.com/rust-lang/rust/issues/63569>
    iterator_try_reduce,                    // #87053 <https://github.com/rust-lang/rust/issues/87053>
    map_try_insert,                         // #82766 <https://github.com/rust-lang/rust/issues/82766>
    new_uninit,                             // #63291 <https://github.com/rust-lang/rust/issues/63291>
    try_trait_v2,                           // #84277 <https://github.com/rust-lang/rust/issues/84277>
    step_trait,                             // #42168 <https://github.com/rust-lang/rust/issues/42168>
    ptr_sub_ptr,                            // #95892 <https://github.com/rust-lang/rust/issues/95892>
    asm_const,
    naked_functions,
    sync_unsafe_cell,
    panic_info_message,
    allocator_api,
    pointer_is_aligned,
    slice_ptr_get,
    strict_provenance,
    core_intrinsics,
    exclusive_range_pattern,
    raw_ref_op,
    let_chains,
    unchecked_math,
    if_let_guard,
    exact_size_is_empty,
    fn_align,
    ptr_as_uninit,
    ptr_metadata,
    control_flow_enum,
    btreemap_alloc,
    inline_const,
    const_option,
    const_option_ext,
    const_trait_impl,
)]
#![forbid(clippy::inline_asm_x86_att_syntax)]
#![deny(clippy::semicolon_if_nothing_returned, clippy::debug_assert_with_mut_call, clippy::float_arithmetic)]
#![warn(
    clippy::cargo,
    clippy::pedantic,
    clippy::undocumented_unsafe_blocks,
    clippy::missing_const_for_fn,
    clippy::cast_lossless
)]
#![allow(
    clippy::enum_glob_use,
    clippy::inline_always,
    clippy::items_after_statements,
    clippy::must_use_candidate,
    clippy::unreadable_literal,
    clippy::wildcard_imports,
    // While ideally this is warned against, the number of situations in which pointer alignment up-casting
    // is acceptable seem to far outweigh the circumstances within the kernel where it is inappropriate.
    clippy::cast_ptr_alignment,
    dead_code
)]
#![cfg_attr(target_arch = "x86_64", feature(abi_x86_interrupt))]

extern crate alloc;
#[macro_use]
extern crate log;
#[macro_use]
extern crate error;

mod acpi;
mod arch;
mod cpu;
mod init;
mod interrupts;
mod logging;
mod mem;
mod panic;
mod rand;
mod task;
mod time;

pub const CORE_STACK_SIZE: u64 = {
    #[cfg(debug_assertions)]
    {
        0x1000000
    }

    #[cfg(not(debug_assertions))]
    {
        0x4000
    }
};

/// ### Safety
///
/// This function should only ever be called by the bootloader.
#[no_mangle]
#[doc(hidden)]
#[allow(clippy::too_many_lines)]
unsafe extern "C" fn _entry() -> ! {
    core::arch::asm!(
        "
        xor rbp, rbp

        call {}
        ",
        sym init::_init,
        options(noreturn)
    )
}
