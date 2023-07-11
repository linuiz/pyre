use crate::{interrupts::exceptions::Exception, task::Registers};
use ia32utils::structures::idt::{InterruptStackFrame, PageFaultErrorCode, SelectorErrorCode};
use libsys::{Address, Virtual};

/// x86_64 exception wrapper type.
#[repr(C)]
#[derive(Debug)]
#[allow(non_camel_case_types, clippy::doc_markdown)]
pub enum ArchException<'a> {
    /// Generated upon an attempt to divide by zero.
    DivideError(&'a InterruptStackFrame, &'a Registers),

    /// Exception generated due to various conditions, outlined within the IA-32 SDM.
    /// Debug registers will be updated to provide context to this exception.
    Debug(&'a InterruptStackFrame, &'a Registers),

    /// Typically caused by unrecoverable RAM or other hardware errors.
    NonMaskable(&'a InterruptStackFrame, &'a Registers),

    /// Occurs when `int3` is called in software.
    Breakpoint(&'a InterruptStackFrame, &'a Registers),

    /// Occurs when the `into` instruction is executed with the `OVERFLOW` bit set in RFlags.
    Overflow(&'a InterruptStackFrame, &'a Registers),

    /// Occurs when the `bound` instruction is executed and fails its check.
    BoundRangeExceeded(&'a InterruptStackFrame, &'a Registers),

    /// Occurs when the processor tries to execute an invalid or undefined opcode.
    InvalidOpcode(&'a InterruptStackFrame, &'a Registers),

    /// Generated when there is no FPU available, but an FPU-reliant instruction is executed.
    DeviceNotAvailable(&'a InterruptStackFrame, &'a Registers),

    /// Occurs when an exception is unhandled or when an exception occurs while the CPU is
    /// trying to call an exception handler.
    DoubleFault(&'a InterruptStackFrame, &'a Registers),

    /// Occurs when an invalid segment selector is referenced as part of a task switch, or as a
    /// result of a control transfer through a gate descriptor, which results in an invalid
    /// stack-segment reference using an SS selector in the TSS
    InvalidTSS(&'a InterruptStackFrame, SelectorErrorCode, &'a Registers),

    /// Occurs when trying to load a segment or gate which has its `PRESENT` bit unset.
    SegmentNotPresent(&'a InterruptStackFrame, SelectorErrorCode, &'a Registers),

    /// Occurs when:
    ///     - Loading a stack-segment referencing a segment descriptor which is not present;
    ///     - Any `push`/`pop` instruction or any instruction using `esp`/`ebp` as a base register
    ///         is executed, while the stack address is not in canonical form;
    ///     - The stack-limit check fails.
    StackSegmentFault(&'a InterruptStackFrame, SelectorErrorCode, &'a Registers),

    /// Occurs when:
    ///     - Segment error (privilege, type, limit, r/w rights).
    ///     - Executing a privileged instruction while CPL isn't supervisor (CPL0)
    ///     - Writing a `1` in a reserved register field or writing invalid value combinations (e.g. `CR0` with `PE` unset and `PG` set).
    ///     - Referencing or accessing a null descriptor.
    GeneralProtectionFault(&'a InterruptStackFrame, SelectorErrorCode, &'a Registers),

    /// Occurs when:
    ///     - A page directory or table entry is not present in physical memory.
    ///     - Attempting to load the instruction TLB with a translation for a non-executable page.
    ///     - A protection cehck (privilege, r/w) failed.
    ///     - A reserved bit in the page directory table or entries is set to 1.
    PageFault(&'a InterruptStackFrame, &'a Registers, PageFaultErrorCode, Address<Virtual>),

    /// Occurs when the `fwait` or `wait` instruction (or any floating point instruction) is executed, and the
    /// following conditions are true:
    ///     - `CR0.NE` is set.
    ///     - An unmasked x87 floating point exception is pending (i.e. the exception bit in the x87 floating point status-word register is set).
    x87FloatingPoint(&'a InterruptStackFrame, &'a Registers),

    /// Occurs when alignment checking is enabled and an unaligned memory data reference is performed.
    ///
    /// REMARK: Alignment checks are only performed when in usermode (CPL3).
    AlignmentCheck(&'a InterruptStackFrame, u64, &'a Registers),

    /// Exception is model-specific and processor implementations are not required to support it.
    ///
    /// REMARK: It uses model-specific registers (MSRs) to provide error information.
    ///         It is disabled by default. Set `CR4.MCE` to enable it.
    MachineCheck(&'a InterruptStackFrame, &'a Registers),

    /* VIRTUALIZATION EXCEPTIONS (not supported) */
    /// Occurs when an unmasked 128-bit media floating-point exception occurs and the `CR4.OSXMMEXCPT` bit
    /// is set. If it is not set, this error condition will trigger an invalid opcode exception instead.
    SimdFlaotingPoint(&'a InterruptStackFrame, &'a Registers),

    /// Occurs only on processors that support setting the `EPT-violation` bit for VM execution control.
    Virtualization(&'a InterruptStackFrame, &'a Registers),

    /// Occurs under several conditions on the `ret`/`iret`/`rstorssp`/`setssbsy` instructions.
    ControlProtection(&'a InterruptStackFrame, &'a Registers),

    HypervisorInjection(&'a InterruptStackFrame, &'a Registers),

    VMMCommunication(&'a InterruptStackFrame, &'a Registers),

    /// Not an exception; it will never be handled by an interrupt handler. It is included here for completeness.
    TripleFault,
}

impl From<ArchException<'_>> for Exception {
    fn from(value: ArchException) -> Self {
        use crate::interrupts::exceptions::{ExceptionKind, PageFaultReason};
        use core::ptr::NonNull;

        match value {
            ArchException::PageFault(isf, _, err, address) => Exception::new(
                ExceptionKind::PageFault {
                    ptr: NonNull::new(address.as_ptr()).unwrap(),
                    reason: if err.contains(PageFaultErrorCode::PROTECTION_VIOLATION) {
                        PageFaultReason::BadPermissions
                    } else {
                        PageFaultReason::NotMapped
                    },
                },
                NonNull::new(isf.instruction_pointer.as_mut_ptr::<u8>()).unwrap(),
                NonNull::new(isf.stack_pointer.as_mut_ptr::<u8>()).unwrap(),
            ),

            _ => todo!(),
        }
    }
}
