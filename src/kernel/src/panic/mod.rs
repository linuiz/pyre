pub mod symbols;

use alloc::string::String;
use core::{fmt::Write, ptr::NonNull};
use libsys::{Address, Virtual};

#[repr(C)]
#[derive(Debug)]
struct StackFrame {
    prev_frame_ptr: Option<NonNull<StackFrame>>,
    return_address: Address<Virtual>,
}

struct StackTracer {
    frame_ptr: Option<NonNull<StackFrame>>,
}

impl StackTracer {
    /// ### Safety
    ///
    /// The provided frame pointer must point to a valid call stack frame.
    const unsafe fn new(frame_ptr: NonNull<StackFrame>) -> Self {
        Self { frame_ptr: Some(frame_ptr) }
    }
}

impl Iterator for StackTracer {
    type Item = Address<Virtual>;

    fn next(&mut self) -> Option<Self::Item> {
        // Safety: Stack frame pointer will be valid if the correct value is provided to `Self::new()`.
        let stack_frame = unsafe { self.frame_ptr?.as_ref() };
        self.frame_ptr = stack_frame.prev_frame_ptr;

        Some(stack_frame.return_address)
    }
}

/// #### Remark
///
/// This function should *never* panic or abort.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    use spin::{Lazy, Mutex};

    static TRACE_BUILDER: Lazy<Mutex<String>> = Lazy::new(|| {
        use crate::mem::alloc::eternal::EternalAllocator;
        use alloc::boxed::Box;

        // Safety: Memory is properly initialized by allocator, and leaked into a `String` with the same capacity.
        unsafe {
            const PANIC_STR_BUFFER_LEN: usize = 0x2000;

            let alloc = Box::<[u8], EternalAllocator>::new_zeroed_slice_in(PANIC_STR_BUFFER_LEN, EternalAllocator)
                .assume_init();
            let string = String::from_raw_parts(Box::leak(alloc).as_mut_ptr(), 0, PANIC_STR_BUFFER_LEN);

            Mutex::new(string)
        }
    });

    let mut trace_builder = TRACE_BUILDER.lock();

    trace_builder.write_fmt(format_args!(
        "KERNEL PANIC (at {}): {}\n",
        info.location().unwrap_or(core::panic::Location::caller()),
        info.message().unwrap_or(&format_args!("no panic message"))
    ));

    stack_trace(&mut trace_builder);

    error!("{trace_builder}");

    trace_builder.clear();

    drop(trace_builder);

    // Safety: It's dead, Jim.
    unsafe { crate::interrupts::halt_and_catch_fire() }
}

fn stack_trace(trace_builder: &mut String) {
    fn write_stack_trace_entry<D: core::fmt::Display>(
        buffer: &mut String,
        entry_num: usize,
        fn_address: Address<Virtual>,
        symbol_name: D,
    ) {
        buffer.write_fmt(format_args!("{entry_num:.<4}0x{:X} {symbol_name:#}\n", fn_address.get())).ok();
    }

    trace_builder.push_str("----------STACK-TRACE---------\n");

    let frame_ptr = {
        #[cfg(target_arch = "x86_64")]
        {
            crate::arch::x86_64::registers::stack::RBP::read() as *const StackFrame
        }
    };

    // Safety: Frame pointer is pulled directly from the frame pointer register.
    if let Some(stack_tracer) = NonNull::new(frame_ptr.cast_mut()).map(|ptr| unsafe { StackTracer::new(ptr) }) {
        for (depth, trace_address) in stack_tracer.enumerate() {
            const SYMBOL_TYPE_FUNCTION: u8 = 2;

            if let Some((_, Some(symbol_name))) = symbols::get(trace_address) {
                if let Ok(demangled) = rustc_demangle::try_demangle(symbol_name) {
                    write_stack_trace_entry(&mut trace_builder, depth, trace_address, demangled);
                } else {
                    write_stack_trace_entry(&mut trace_builder, depth, trace_address, symbol_name);
                }
            } else {
                write_stack_trace_entry(&mut trace_builder, depth, trace_address, "!!! no function found !!!");
            }
        }
    } else {
        trace_builder.push_str("No base pointer; stack trace empty.\n");
    }

    trace_builder.push_str("----------STACK-TRACE----------\n");
}
