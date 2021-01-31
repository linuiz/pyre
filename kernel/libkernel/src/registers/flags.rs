use bitflags::bitflags;

bitflags! {
    pub struct RFlags: u64 {
        /// Processor feature identification flag.
        ///
        /// If this flag is modifiable, the CPU supports CPUID.
        const ID = 1 << 21;
        ///Indicates that an external, maskable interrupt is pending.
        ///
        /// Used when virtual-8086 mode extensions (CR4.VME) or protected-mode virtual
        /// interrupts (CR4.PVI) are activated.
        const VIRTUAL_INTERRUPT_PENDING = 1 << 20;
        /// Virtual image of the INERRUPT_FLAG bit.
        ///
        /// Used when virtual-8086 mode extensions (CR4.VME) or protected-mode virtual
        /// interrupts (CR4.PVI) are activated.
        const VIRTUAL_INTERRUPT = 1 << 19;
        /// Enable automatic alignment-checking if the CR0.AM is set. Only works
        /// if CPL is 3.
        const ALIGNMENT_CHECK = 1 << 18;
        /// Enable the virtual-8086 mode.
        const VIRTUAL_8086_MODE = 1 << 17;
        /// Allows to retsrat an instruction following an instruction breakpoint.
        const RESUME_FLAG = 1 << 16;
        /// Used by `iret` in hardware task switch mode to determine if current task is nested.
        const NESTED_TASK = 1 << 14;
        /// The high bit of the I/O Privilege Level field.
        ///
        /// Specifies the privelege level required for executing I/O address-space instructions.
        const IOPL_HIGH = 1<< 13;
        /// The low bit of the I/O Privilege Level field.
        ///
        /// Specifies the privilege level required for executing the I/O address-space instructions.
        const IOPL_LOW = 1 << 12;
        /// Set by hardware to indicate that the sign bit of the result of the last signed integer
        /// operation differs from the source operands.
        const OVERFLOW_FLAG = 1 << 11;
        /// Determines the order in which strings are processes.
        const DIRECTION_FLAG = 1 << 10;
        /// Enable interrupts.
        const INTERRUPT_FLAG = 1 << 9;
        /// Enable single-step mode for debugging.
        const TRAP_FLAG = 1 << 8;
        /// Set by hardware if the last arithmetic operation resulted in a negative value.
        const SIGN_FLAG = 1 << 7;
        /// Set by hardware if last arithmetic operation resulted in a zero value.
        const ZERO_FLAG = 1 << 6;
        /// Set by hardware if the last arithmetic operation generated a carry out of bit 3 of the result.
        const AUXILIARY_CARRY_FLAG = 1 << 4;
        /// Set by hardware if the last result has an even number of 1 bits (only for some operations).
        const PARITY_FLAG = 1 << 2;
        /// Set by hardware if the last arithmetic operation generated a carry out of the mos-significant
        /// bit of the result.
        const CARRY_FLAG = 1 << 0;
    }
}

impl RFlags {
    pub fn read() -> Self {
        Self::from_bits_truncate(Self::read_raw())
    }

    fn read_raw() -> u64 {
        let result: u64;

        unsafe {
            asm!("pushf", "pop {}", out(reg) result);
        }

        result
    }

    pub unsafe fn write(flags: Self, set: bool) {
        let reserved_bits = Self::read_raw() & !Self::all().bits();
        let mut old_flags = Self::read();
        old_flags.set(flags, set);
        let rflags_bits = old_flags.bits() | reserved_bits;

        asm!("push {}", "popf", in(reg) rflags_bits, options(preserves_flags));
    }
}
