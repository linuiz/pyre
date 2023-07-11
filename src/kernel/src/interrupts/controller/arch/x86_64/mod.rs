mod idt;
mod tss;

mod apic;
use apic::*;

mod vector;
pub use vector::*;

use crate::interrupts::controller::Controller;
use alloc::boxed::Box;
use core::{
    num::{NonZeroU16, NonZeroU64},
    time::Duration,
};
use ia32utils::structures::{idt::InterruptDescriptorTable, tss::TaskStateSegment};

/// Defines set indexes which specified interrupts will use for stacks.
#[repr(usize)]
#[derive(Debug, Clone, Copy)]
pub enum StackTableIndex {
    Debug = 0,
    NonMaskable = 1,
    DoubleFault = 2,
    MachineCheck = 3,
}

/// ### Safety
///
/// Initializing a new controller on a core that already has a controller can potentially cause UB.
///
/// 1. Caller must ensure this function is called only once per core.
/// 2. Caller must ensure the controller lives for the lifetime of the system.
pub unsafe fn new_controller() -> Box<dyn Controller> {
    use core::num::NonZeroUsize;
    use core::ptr::NonNull;
    use ia32utils::VirtAddr;

    fn allocate_tss_stack() -> VirtAddr {
        const TSS_STACK_SIZE: NonZeroUsize = NonZeroUsize::new(0x16000).unwrap();

        let stack: Box<[core::mem::MaybeUninit<u8>]> = Box::new_uninit_slice(TSS_STACK_SIZE.get());
        VirtAddr::from_ptr(Box::leak(stack).as_ptr_range().end)
    }

    // init Interrupt Descriptor Table
    let mut idt = Box::new(InterruptDescriptorTable::new());
    idt::set_exception_handlers(&mut idt);
    idt::set_stub_handlers(&mut idt);
    // Safety: IDT resides in the controller, which should not be destroyed.
    unsafe { idt.load_unsafe() };

    // init Task State Segment
    let mut tss = Box::new(TaskStateSegment::new());
    // TODO guard pages for these stacks
    tss.privilege_stack_table[0] = allocate_tss_stack();
    tss.interrupt_stack_table[StackTableIndex::Debug as usize] = allocate_tss_stack();
    tss.interrupt_stack_table[StackTableIndex::NonMaskable as usize] = allocate_tss_stack();
    tss.interrupt_stack_table[StackTableIndex::DoubleFault as usize] = allocate_tss_stack();
    tss.interrupt_stack_table[StackTableIndex::MachineCheck as usize] = allocate_tss_stack();
    tss::load_local(tss::ptr_as_descriptor(NonNull::new(tss.as_mut()).unwrap()));

    // init Advanced Programmable Interrupt Controller
    let apic = Apic::new_init().expect("failed to initialize interrupt controller");

    let mut controller = Box::new(ControllerImpl { idt, tss, apic, timer_freq_interval: None });
    controller.reset();

    controller
}

pub struct ControllerImpl {
    idt: Box<InterruptDescriptorTable>,
    tss: Box<TaskStateSegment>,
    apic: Apic,
    timer_freq_interval: Option<NonZeroU64>,
}

impl Controller for ControllerImpl {
    unsafe fn reset(&mut self) {
        use bit_field::BitField;

        let apic = &mut self.apic;

        apic.sw_disable();

        apic.write_register(Register::TPR, 0x0);
        apic.write_register(
            Register::SPR,
            *apic.read_register(Register::SPR).set_bits(0..8, u32::from(Vector::AutoEoi)),
        );

        apic.sw_enable();

        apic.set_vector::<Timer>(Vector::Timer.into());
        apic.set_masked::<Exception>(true);
        apic.set_vector::<Exception>(Vector::Error.into());
        apic.set_masked::<Exception>(true);
        apic.set_vector::<Performance>(Vector::Performance.into());
        apic.set_masked::<Performance>(true);
        apic.set_vector::<Thermal>(Vector::Thermal.into());
        apic.set_masked::<Thermal>(true);
        // IA32 SDM specifies that after a software disable, all local vectors
        // are masked, so we need to re-enable the LINTx vectors.
        apic.set_masked::<LINT0>(false);
        apic.set_vector::<LINT0>(Vector::AutoEoi.into());
        apic.set_masked::<LINT1>(false);
        apic.set_vector::<LINT1>(Vector::AutoEoi.into());
    }

    unsafe fn enable(&mut self) {
        self.apic.sw_enable();
    }

    unsafe fn disable(&mut self) {
        self.apic.sw_disable();
    }

    unsafe fn enable_timer(&mut self) {
        self.apic.set_masked::<Timer>(true);
    }

    unsafe fn disable_timer(&mut self) {
        self.apic.set_masked::<Timer>(false);
    }

    unsafe fn set_timer_frequency(&mut self, desired_frequency: NonZeroU16) {
        use crate::arch::x86_64;

        let apic = &mut self.apic;

        const WAIT: u64 = 10;
        const FACTOR: u64 = 1000 / WAIT;

        // Configure APIC timer in most advanced mode.
        let timer_interval = {
            if x86_64::cpuid::FEATURE_INFO.has_tsc() && x86_64::cpuid::FEATURE_INFO.has_tsc_deadline() {
                apic.set_timer_mode(TimerMode::TscDeadline);

                let timer_frequency = {
                    if let Some(freq_info) = x86_64::cpuid::CPUID.get_processor_frequency_info() {
                        let bus_freq: u64 = freq_info.bus_frequency().into();
                        let base_freq: u64 = freq_info.processor_base_frequency().into();
                        let max_freq: u64 = freq_info.processor_max_frequency().into();

                        bus_freq / (base_freq * max_freq)
                    } else {
                        libsys::do_once!({
                            trace!("Processors do not support TSC frequency reporting via CPUID.");
                        });

                        apic.sw_enable();
                        apic.set_masked::<Timer>(true);

                        let start_tsc = core::arch::x86_64::_rdtsc();
                        crate::time::clock::SYSTEM.spin_wait(Duration::from_millis(WAIT));
                        let end_tsc = core::arch::x86_64::_rdtsc();

                        (end_tsc - start_tsc) * FACTOR
                    }
                };

                timer_frequency / u64::from(desired_frequency.get())
            } else {
                apic.sw_enable();
                apic.set_timer_divisor(TimerDivisor::Div1);
                apic.set_masked::<Timer>(true);
                apic.set_timer_mode(TimerMode::OneShot);

                let timer_frequency = {
                    apic.set_timer_initial_count(u32::MAX);
                    crate::time::clock::SYSTEM.spin_wait(Duration::from_millis(10));
                    let timer_count = apic.get_timer_current_count();

                    (u32::MAX - timer_count) * u32::try_from(FACTOR).unwrap()
                };

                // Ensure we reset the APIC timer to avoid any errant interrupts.
                apic.set_timer_initial_count(0);

                u64::from(timer_frequency) / u64::from(desired_frequency.get())
            }
        };

        self.timer_freq_interval = Some(NonZeroU64::new(timer_interval).unwrap());
    }

    unsafe fn set_timer_wait(&mut self, wait_factor: NonZeroU16) {
        let apic = &mut self.apic;
        let timer_interval = self.timer_freq_interval.expect("timer frequency has not been configured").get();
        let wait_factor = u64::from(wait_factor.get());
        let total_wait_ticks = timer_interval * wait_factor;

        match apic.get_timer_mode() {
            // Safety: Control flow expects timer initial count to be set.
            apic::TimerMode::OneShot => unsafe {
                apic.set_timer_initial_count(
                    total_wait_ticks.try_into().expect("wait factor overflowed timer capacity"),
                );
            },

            // Safety: Control flow expects the TSC deadline to be set.
            apic::TimerMode::TscDeadline => unsafe {
                crate::arch::x86_64::registers::msr::IA32_TSC_DEADLINE::set(
                    core::arch::x86_64::_rdtsc() + total_wait_ticks,
                );
            },

            apic::TimerMode::Periodic => unimplemented!(),
        }
    }

    fn end_interrupt(&mut self) {
        self.apic.end_of_interrupt();
    }
}
