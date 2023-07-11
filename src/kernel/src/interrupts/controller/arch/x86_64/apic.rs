#![allow(non_upper_case_globals, clippy::doc_markdown)]

use crate::mem::paging;
use bit_field::BitField;
use core::{ops::Range, ptr::NonNull};
use libsys::{Address, Frame};
use msr::IA32_APIC_BASE;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::upper_case_acronyms)]
pub enum DeliveryMode {
    Fixed = 0b000,
    LowPriority = 0b001,
    SMI = 0b010,
    NMI = 0b100,
    INIT = 0b101,
    StartUp = 0b110,
    ExtINT = 0b111,
}

/// Various valid modes for APIC timer to operate in.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerMode {
    OneShot = 0b00,
    Periodic = 0b01,
    TscDeadline = 0b10,
}

impl TryFrom<u32> for TimerMode {
    type Error = u32;

    fn try_from(value: u32) -> core::result::Result<Self, Self::Error> {
        match value {
            0b00 => Ok(Self::OneShot),
            0b01 => Ok(Self::Periodic),
            0b10 => Ok(Self::TscDeadline),
            value => Err(value),
        }
    }
}

/// Divisor for APIC timer to use when not in [`TimerMode::TscDeadline`].
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerDivisor {
    Div2 = 0b0000,
    Div4 = 0b0001,
    Div8 = 0b0010,
    Div16 = 0b0011,
    Div32 = 0b1000,
    Div64 = 0b1001,
    Div128 = 0b1010,
    Div1 = 0b1011,
}

impl TimerDivisor {
    /// Converts the given [`TimerDivisor`] to its numeric counterpart.
    pub const fn as_divide_value(self) -> u8 {
        match self {
            TimerDivisor::Div2 => 2,
            TimerDivisor::Div4 => 4,
            TimerDivisor::Div8 => 8,
            TimerDivisor::Div16 => 16,
            TimerDivisor::Div32 => 32,
            TimerDivisor::Div64 => 64,
            TimerDivisor::Div128 => 128,
            TimerDivisor::Div1 => 1,
        }
    }
}

bitflags::bitflags! {
    #[repr(transparent)]
    pub struct ErrorStatusFlags : u32 {
        const SEND_CHECKSUM_ERROR = 1 << 0;
        const RECEIVE_CHECKSUM_ERROR = 1 << 1;
        const SEND_ACCEPT_ERROR = 1 << 2;
        const RECEIVE_ACCEPT_ERROR = 1 << 3;
        const REDIRECTABLE_IPI = 1 << 4;
        const SENT_ILLEGAL_VECTOR = 1 << 5;
        const RECEIVED_ILLEGAL_VECTOR = 1 << 6;
        const ILLEGAL_REGISTER_ADDRESS = 1 << 7;
    }
}

#[derive(Debug, Clone, Copy)]
pub struct InterruptCommand {
    apic_id: u32,
    cmd: u32,
}

impl InterruptCommand {
    pub fn new(vector: u8, apic_id: u32, delivery_mode: DeliveryMode, is_logical: bool, is_assert: bool) -> Self {
        Self {
            apic_id,
            cmd: *0u32
                .set_bits(0..8, vector.into())
                .set_bits(8..11, delivery_mode as u32)
                .set_bit(11, is_logical)
                .set_bit(14, is_assert),
        }
    }

    #[inline]
    pub fn new_init(apic_id: u32) -> Self {
        Self::new(0, apic_id, DeliveryMode::INIT, false, true)
    }

    #[inline]
    pub fn new_sipi(vector: u8, apic_id: u32) -> Self {
        Self::new(vector, apic_id, DeliveryMode::StartUp, false, true)
    }

    #[inline]
    pub const fn get_id(self) -> u32 {
        self.apic_id
    }

    #[inline]
    pub const fn get_cmd(self) -> u32 {
        self.cmd
    }
}

/// Various APIC registers, valued as their base register index.
#[repr(u8)]
#[derive(Clone, Copy)]
#[allow(clippy::upper_case_acronyms, non_camel_case_types)]
pub enum Register {
    ID = 0x02,
    VERSION = 0x03,
    TPR = 0x08,
    PPR = 0x0A,
    EOI = 0x0B,
    LDR = 0x0C,
    SPR = 0x0F,
    ISR0 = 0x10,
    ISR32 = 0x11,
    ISR64 = 0x12,
    ISR96 = 0x13,
    ISR128 = 0x14,
    ISR160 = 0x15,
    ISR192 = 0x16,
    ISR224 = 0x17,
    TMR0 = 0x18,
    TMR32 = 0x19,
    TMR64 = 0x1A,
    TMR96 = 0x1B,
    TMR128 = 0x1C,
    TMR160 = 0x1D,
    TMR192 = 0x1E,
    TMR224 = 0x1F,
    IRR0 = 0x20,
    IRR32 = 0x21,
    IRR64 = 0x22,
    IRR96 = 0x23,
    IRR128 = 0x24,
    IRR160 = 0x25,
    IRR192 = 0x26,
    IRR224 = 0x27,
    ERR = 0x28,
    ICRL = 0x30,
    ICRH = 0x31,
    LVT_TIMER = 0x32,
    LVT_THERMAL = 0x33,
    LVT_PERF = 0x34,
    LVT_LINT0 = 0x35,
    LVT_LINT1 = 0x36,
    LVT_ERR = 0x37,
    TIMER_INT_CNT = 0x38,
    TIMER_CUR_CNT = 0x39,
    TIMER_DIVISOR = 0x3E,
    SELF_IPI = 0x3F,
}

impl Register {
    /// Translates this APIC register to its respective xAPIC memory offset.
    #[inline]
    pub const fn xapic_offset(self) -> usize {
        (self as usize) * 0x10
    }

    /// Translates this APIC register to its respective x2APIC MSR address.
    #[inline]
    pub const fn x2apic_msr(self) -> u32 {
        x2APIC_BASE_MSR_ADDR + (self as u32)
    }
}

errorgen! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Error {
        Paging { err: paging::Error } => Some(err),
        HwDisabled => None,
        NoX2ApicSupport => None,
    }
}

pub const xAPIC_BASE_ADDR: usize = 0xFEE00000;
pub const x2APIC_BASE_MSR_ADDR: u32 = 0x800;

#[repr(transparent)]
pub struct Apic(NonNull<u32>);

// Safety: Apic utilizes HHDM.
unsafe impl Send for Apic {}
// Safety: Apic uses MMIO with per-core access.
unsafe impl Sync for Apic {}

impl Apic {
    const LVT_VECTOR_BITS: Range<usize> = 0..8;
    const LVT_DELIVERY_MODE_BITS: Range<usize> = 8..11;
    const LVT_INTERRUPTED_BIT: usize = 12;
    const LVT_MASKED_BIT: usize = 16;
    const LVT_TIMER_MODE_BITS: Range<usize> = 17..19;

    #[allow(clippy::similar_names)]
    pub fn new_init() -> Result<Self> {
        if !IA32_APIC_BASE::get_hw_enabled() || IA32_APIC_BASE::get_is_x2_mode() {
            return Err(Error::HwDisabled);
        }

        use crate::mem::{with_kmapper, HHDM};
        use paging::{Error as PagingError, TableDepth, TableEntryFlags};

        let xapic_addr = usize::try_from(IA32_APIC_BASE::get_base_address()).unwrap();
        let xapic_addr = Address::<Frame>::new(xapic_addr).unwrap();
        let xapic_hhdm_addr = HHDM.offset(xapic_addr).unwrap();

        with_kmapper(|mapper| {
            match mapper.map(xapic_hhdm_addr, TableDepth::min(), xapic_addr, true, TableEntryFlags::MMIO) {
                Ok(_) | Err(PagingError::AllocError) => Ok(()),
                Err(err) => Err(Error::Paging { err }),
            }
        })?;

        Ok(Self(NonNull::new(xapic_hhdm_addr.as_ptr().cast()).unwrap()))
    }

    #[inline]
    fn get_xapic_ptr(&self, register: Register) -> NonNull<u32> {
        unsafe { NonNull::new_unchecked(self.0.as_ptr().byte_add(register.xapic_offset())) }
    }

    /// Reads the given register from the local APIC.
    #[inline]
    pub fn read_register(&self, register: Register) -> u32 {
        // Safety: Address provided for xAPIC mapping is required to be valid.
        unsafe { self.get_xapic_ptr(register).as_ptr().read_volatile() }
    }

    /// ### Safety
    ///
    /// Writing an invalid value to a register is undefined behaviour.
    #[inline]
    pub unsafe fn write_register(&mut self, register: Register, value: u32) {
        // Safety: Address provided for xAPIC mapping is required to be valid.
        unsafe {
            self.get_xapic_ptr(register).as_ptr().write_volatile(value);
        }
    }

    /// ### Safety
    ///
    /// Given the amount of external contexts that could potentially rely on the APIC, enabling it
    /// has the oppurtunity to affect those contexts in undefined ways.
    #[inline]
    pub unsafe fn sw_enable(&mut self) {
        self.write_register(Register::SPR, *self.read_register(Register::SPR).set_bit(8, true));
    }

    /// ### Safety
    ///
    /// Given the amount of external contexts that could potentially rely on the APIC, disabling it
    /// has the oppurtunity to affect those contexts in undefined ways.
    #[inline]
    pub unsafe fn sw_disable(&mut self) {
        self.write_register(Register::SPR, *self.read_register(Register::SPR).set_bit(8, false));
    }

    #[inline]
    pub fn get_id(&self) -> u32 {
        self.read_register(Register::ID).get_bits(24..32)
    }

    #[inline]
    pub fn get_version(&self) -> u32 {
        self.read_register(Register::VERSION)
    }

    // TODO maybe unsafe?
    #[inline]
    pub fn end_of_interrupt(&mut self) {
        unsafe { self.write_register(Register::EOI, 0x0) };
    }

    #[inline]
    pub fn get_error_status(&self) -> ErrorStatusFlags {
        ErrorStatusFlags::from_bits_truncate(self.read_register(Register::ERR))
    }

    /// ### Safety
    ///
    /// An invalid or unexpcted interrupt command could potentially put the core in an unusable state.
    #[inline]
    pub unsafe fn send_int_cmd(&mut self, interrupt_command: InterruptCommand) {
        self.write_register(Register::ICRL, interrupt_command.get_id());
        self.write_register(Register::ICRH, interrupt_command.get_cmd());
    }

    /// ### Safety
    ///
    /// The timer divisor directly affects the tick rate and interrupt rate of the
    /// internal local timer clock. Thus, changing the divisor has the potential to
    /// cause the same sorts of UB that [`set_timer_initial_count`] can cause.
    #[inline]
    pub unsafe fn set_timer_divisor(&mut self, divisor: TimerDivisor) {
        self.write_register(Register::TIMER_DIVISOR, divisor.as_divide_value().into());
    }

    /// ### Safety
    ///
    /// Setting the initial count of the timer resets its internal clock. This can lead
    /// to a situation where another context is awaiting a specific clock duration, but
    /// is instead interrupted later than expected.
    #[inline]
    pub unsafe fn set_timer_initial_count(&mut self, count: u32) {
        self.write_register(Register::TIMER_INT_CNT, count);
    }

    #[inline]
    pub fn get_timer_current_count(&self) -> u32 {
        self.read_register(Register::TIMER_CUR_CNT)
    }

    #[inline]
    #[allow(clippy::cast_possible_truncation)]
    pub fn get_vector<Kind: LocalVectorKind>(&self) -> u8 {
        self.read_register(Kind::REGISTER).get_bits(Self::LVT_VECTOR_BITS) as u8
    }

    #[inline]
    #[allow(clippy::cast_possible_truncation)]
    pub fn set_vector<Kind: LocalVectorKind>(&mut self, vector: u8) {
        assert!(vector >= 32, "interrupt vectors 0..32 are reserved");

        // Safety: Provided register format is valid.
        unsafe {
            self.write_register(
                Kind::REGISTER,
                *self.read_register(Kind::REGISTER).set_bits(Self::LVT_VECTOR_BITS, vector.into()),
            );
        }
    }

    #[inline]
    pub fn get_interrupted<Kind: LocalVectorKind>(&self) -> bool {
        self.read_register(Kind::REGISTER).get_bit(Self::LVT_INTERRUPTED_BIT)
    }

    #[inline]
    pub fn get_masked<Kind: LocalVectorKind>(&self) -> bool {
        self.read_register(Kind::REGISTER).get_bit(Self::LVT_MASKED_BIT)
    }

    pub fn set_masked<Kind: LocalVectorKind>(&mut self, mask: bool) {
        // Safety: Provided register format is valid.
        unsafe {
            self.write_register(
                Kind::REGISTER,
                *self.read_register(Kind::REGISTER).set_bit(Self::LVT_MASKED_BIT, mask),
            );
        }
    }

    pub fn set_delivery_mode<Kind: LocalVectorKind>(&mut self, mode: DeliveryMode) {
        unsafe {
            self.write_register(
                Kind::REGISTER,
                *self.read_register(Kind::REGISTER).set_bits(Self::LVT_DELIVERY_MODE_BITS, mode as u32),
            );
        }
    }

    #[inline]
    pub fn get_timer_mode(&self) -> TimerMode {
        match self.read_register(Timer::REGISTER).get_bits(Self::LVT_TIMER_MODE_BITS) {
            0b00 => TimerMode::OneShot,
            0b01 => TimerMode::Periodic,
            0b10 => TimerMode::TscDeadline,
            _ => unreachable!(),
        }
    }

    /// ### Safety
    ///
    /// Setting the mode of the timer may result in undefined behaviour if switching modes while
    /// the APIC is currently active and ticking (or otherwise expecting the timer to behave in
    /// a particular, pre-defined fashion).
    pub unsafe fn set_timer_mode(&mut self, mode: TimerMode) -> &Self {
        let tsc_dl_support = core::arch::x86_64::__cpuid(0x1).ecx.get_bit(24);

        assert!(mode != TimerMode::TscDeadline || tsc_dl_support, "TSC deadline is not supported on this CPU.");

        self.write_register(
            Timer::REGISTER,
            *self.read_register(Timer::REGISTER).set_bits(Self::LVT_TIMER_MODE_BITS, mode as u32),
        );

        if tsc_dl_support {
            // IA32 SDM instructs utilizing the `mfence` instruction to ensure all writes to the IA32_TSC_DEADLINE
            // MSR are serialized *after* the APIC timer mode switch (`wrmsr` to `IA32_TSC_DEADLINE` is non-serializing).
            core::arch::asm!("mfence", options(nostack, nomem, preserves_flags));
        }

        self
    }
}

pub trait LocalVectorKind {
    const REGISTER: Register;
}

pub trait GenericVectorVariant: LocalVectorKind {}

pub struct Timer;
impl LocalVectorKind for Timer {
    const REGISTER: Register = Register::LVT_TIMER;
}

pub struct LINT0;
impl LocalVectorKind for LINT0 {
    const REGISTER: Register = Register::LVT_LINT0;
}
impl GenericVectorVariant for LINT0 {}

pub struct LINT1;
impl LocalVectorKind for LINT1 {
    const REGISTER: Register = Register::LVT_LINT1;
}
impl GenericVectorVariant for LINT1 {}

pub struct Performance;
impl LocalVectorKind for Performance {
    const REGISTER: Register = Register::LVT_PERF;
}
impl GenericVectorVariant for Performance {}

pub struct Thermal;
impl LocalVectorKind for Thermal {
    const REGISTER: Register = Register::LVT_THERMAL;
}
impl GenericVectorVariant for Thermal {}

pub struct Exception;
impl LocalVectorKind for Exception {
    const REGISTER: Register = Register::LVT_ERR;
}
