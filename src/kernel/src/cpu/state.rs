use crate::{interrupts::exceptions::Exception, interrupts::InterruptCell, task::Scheduler};
use alloc::boxed::Box;
use core::{cell::UnsafeCell, num::NonZeroU64, ptr::NonNull, sync::atomic::AtomicBool};

pub(self) const US_PER_SEC: u32 = 1000000;
pub(self) const US_WAIT: u32 = 10000;
pub(self) const US_FREQ_FACTOR: u32 = US_PER_SEC / US_WAIT;

errorgen! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Error {
        NotInitialized => None
    }
}

pub const STACK_SIZE: usize = 0x10000;

#[repr(C)]
struct State {
    core_id: u32,
    scheduler: InterruptCell<Scheduler>,

    #[cfg(target_arch = "x86_64")]
    idt: Box<crate::arch::x86_64::structures::idt::InterruptDescriptorTable>,
    #[cfg(target_arch = "x86_64")]
    tss: Box<crate::arch::x86_64::structures::tss::TaskStateSegment>,

    #[cfg(target_arch = "x86_64")]
    apic: apic::Apic,

    timer_interval: Option<NonZeroU64>,

    catch_exception: AtomicBool,
    exception: UnsafeCell<Option<Exception>>,
}

pub const SYSCALL_STACK_SIZE: usize = 0x40000;

pub enum ExceptionCatcher {
    Caught(Exception),
    Await,
    Idle,
}

/// Initializes the core-local state structure.
///
/// ### Safety
///
/// This function invariantly assumes it will only be called once.
#[allow(clippy::too_many_lines)]
pub unsafe fn init(timer_frequency: u16) {
    #[cfg(target_arch = "x86_64")]
    let idt = {
        use crate::arch::x86_64::structures::idt;

        let mut idt = Box::new(idt::InterruptDescriptorTable::new());

        idt::set_exception_handlers(&mut idt);
        idt::set_stub_handlers(&mut idt);
        idt.load_unsafe();

        idt
    };

    #[cfg(target_arch = "x86_64")]
    let tss = {
        use crate::arch::x86_64::structures::{idt::StackTableIndex, tss};
        use core::num::NonZeroUsize;
        use ia32utils::VirtAddr;

        fn allocate_tss_stack() -> VirtAddr {
            const TSS_STACK_SIZE: NonZeroUsize = NonZeroUsize::new(0x16000).unwrap();

            let stack: Box<[core::mem::MaybeUninit<u8>]> = Box::new_uninit_slice(TSS_STACK_SIZE.get());
            VirtAddr::from_ptr(Box::leak(stack).as_ptr_range().end)
        }

        info!("1");
        let mut tss = Box::new(tss::TaskStateSegment::new());
        info!("1");
        // TODO guard pages for these stacks
        tss.privilege_stack_table[0] = allocate_tss_stack();
        info!("2");
        tss.interrupt_stack_table[StackTableIndex::Debug as usize] = allocate_tss_stack();
        info!("3");
        tss.interrupt_stack_table[StackTableIndex::NonMaskable as usize] = allocate_tss_stack();
        info!("4");
        tss.interrupt_stack_table[StackTableIndex::DoubleFault as usize] = allocate_tss_stack();
        info!("5");
        tss.interrupt_stack_table[StackTableIndex::MachineCheck as usize] = allocate_tss_stack();
        info!("6");

        tss::load_local(tss::ptr_as_descriptor(NonNull::new(tss.as_mut()).unwrap()));
        info!("7");

        tss
    };

    let mut state = Box::new(State {
        core_id: crate::cpu::read_id(),
        scheduler: InterruptCell::new(Scheduler::new(false)),

        #[cfg(target_arch = "x86_64")]
        idt,
        #[cfg(target_arch = "x86_64")]
        tss,

        #[cfg(target_arch = "x86_64")]
        apic: apic::Apic::new(Some(|address: usize| crate::mem::HHDM.ptr().add(address))).unwrap(),

        timer_interval: None,

        catch_exception: AtomicBool::new(false),
        exception: UnsafeCell::new(None),
    });

    /* init APIC */
    {
        use crate::{arch::x86_64, interrupts::Vector};

        let apic = &mut state.apic;

        // Bring APIC to known state.
        apic.software_reset(255, 254, 253);
        apic.get_timer().set_vector(Vector::Timer as u8);
        apic.get_error().set_vector(Vector::Error as u8).set_masked(false);
        apic.get_performance().set_vector(Vector::Performance as u8).set_masked(true);
        apic.get_thermal_sensor().set_vector(Vector::Thermal as u8).set_masked(true);

        // Configure APIC timer in most advanced mode.
        let timer_interval = if x86_64::cpuid::FEATURE_INFO.has_tsc() && x86_64::cpuid::FEATURE_INFO.has_tsc_deadline()
        {
            apic.get_timer().set_mode(apic::TimerMode::TscDeadline);

            let frequency = x86_64::cpuid::CPUID.get_processor_frequency_info().map_or_else(
                || {
                    libsys::do_once!({
                        trace!("Processors do not support TSC frequency reporting via CPUID.");
                    });

                    apic.sw_enable();
                    apic.get_timer().set_masked(true);

                    let start_tsc = core::arch::x86_64::_rdtsc();
                    crate::time::SYSTEM_CLOCK.spin_wait_us(US_WAIT);
                    let end_tsc = core::arch::x86_64::_rdtsc();

                    (end_tsc - start_tsc) * u64::from(US_FREQ_FACTOR)
                },
                |info| {
                    u64::from(info.bus_frequency())
                        / (u64::from(info.processor_base_frequency()) * u64::from(info.processor_max_frequency()))
                },
            );

            frequency / u64::from(timer_frequency)
        } else {
            apic.sw_enable();
            apic.set_timer_divisor(apic::TimerDivisor::Div1);
            apic.get_timer().set_masked(true).set_mode(apic::TimerMode::OneShot);

            let frequency = {
                apic.set_timer_initial_count(u32::MAX);
                crate::time::SYSTEM_CLOCK.spin_wait_us(US_WAIT);
                let timer_count = apic.get_timer_current_count();

                (u32::MAX - timer_count) * US_FREQ_FACTOR
            };

            // Ensure we reset the APIC timer to avoid any errant interrupts.
            apic.set_timer_initial_count(0);

            u64::from(frequency / u32::from(timer_frequency))
        };

        state.timer_interval = NonZeroU64::new(timer_interval);
    }

    let state_address = Box::into_raw(state).addr();

    #[cfg(target_arch = "x86_64")]
    crate::arch::x86_64::registers::msr::IA32_KERNEL_GS_BASE::write(state_address as u64);
}

fn get_state_ptr() -> Result<NonNull<State>> {
    let kernel_gs_usize = usize::try_from(crate::arch::x86_64::registers::msr::IA32_KERNEL_GS_BASE::read()).unwrap();
    NonNull::new(kernel_gs_usize as *mut State).ok_or(Error::NotInitialized)
}

fn get_state() -> Result<&'static State> {
    // Safety: If the pointer is non-null, the kernel guarantees it will be initialized.
    unsafe { get_state_ptr().map(|ptr| ptr.as_ref()) }
}

fn get_state_mut() -> Result<&'static mut State> {
    // Safety: If the pointer is non-null, the kernel guarantees it will be initialized.
    unsafe { get_state_ptr().map(|mut ptr| ptr.as_mut()) }
}

/// Returns the generated ID for the local core.
pub fn get_core_id() -> Result<u32> {
    get_state().map(|state| state.core_id)
}

pub unsafe fn begin_scheduling() -> Result<()> {
    // Enable scheduler ...
    with_scheduler(|scheduler| {
        assert!(!scheduler.is_enabled());
        scheduler.enable();
    });

    // Enable APIC timer ...
    let apic = &mut get_state_mut()?.apic;
    assert!(apic.get_timer().get_masked());
    // Safety: Calling `begin_scheduling` implies this state change is expected.
    unsafe {
        apic.get_timer().set_masked(false);
    }

    // Safety: Calling `begin_scheduling` implies this function is expected to be called.
    unsafe {
        set_preemption_wait(core::num::NonZeroU16::MIN)?;
    }

    Ok(())
}

pub fn with_scheduler<O>(func: impl FnOnce(&mut crate::task::Scheduler) -> O) -> O {
    let state = get_state_mut().unwrap();
    state.scheduler.with_mut(func)
}

/// Ends the current interrupt context for the interrupt controller.
///
/// On platforms that don't require an EOI, this is a no-op.
pub unsafe fn end_of_interrupt() -> Result<()> {
    #[cfg(target_arch = "x86_64")]
    get_state().map(|state| state.apic.end_of_interrupt())?;

    Ok(())
}

/// ### Safety
///
/// Caller must ensure that setting a new preemption wait will not cause undefined behaviour.
pub unsafe fn set_preemption_wait(interval_wait: core::num::NonZeroU16) -> Result<()> {
    let state = get_state_mut()?;
    let timer_interval = state.timer_interval.unwrap();

    #[cfg(target_arch = "x86_64")]
    {
        let apic = &mut state.apic;

        match apic.get_timer().get_mode() {
            // Safety: Control flow expects timer initial count to be set.
            apic::TimerMode::OneShot => unsafe {
                let final_count = timer_interval.get() * u64::from(interval_wait.get());
                apic.set_timer_initial_count(final_count.try_into().unwrap_or(u32::MAX));
            },

            // Safety: Control flow expects the TSC deadline to be set.
            apic::TimerMode::TscDeadline => unsafe {
                crate::arch::x86_64::registers::msr::IA32_TSC_DEADLINE::set(
                    core::arch::x86_64::_rdtsc() + (timer_interval.get() * u64::from(interval_wait.get())),
                );
            },

            apic::TimerMode::Periodic => unimplemented!(),
        }
    }

    Ok(())
}

// pub fn provide_exception<T: Into<Exception>>(exception: T) -> core::result::Result<(), T> {
//     let state = get_state_mut();
//     if state.catch_exception.load(Ordering::Relaxed) {
//         let exception_cell = state.exception.get_mut();

//         debug_assert!(exception_cell.is_none());
//         *exception_cell = Some(exception.into());
//         Ok(())
//     } else {
//         Err(exception)
//     }
// }

// /// ### Safety
// ///
// /// Caller must ensure `do_func` is effectively stackless, since no stack cleanup will occur on an exception.
// pub unsafe fn do_catch<T>(do_func: impl FnOnce() -> T) -> core::result::Result<T, Exception> {
//     let state = get_state_mut();

//     debug_assert!(state.exception.get_mut().is_none());

//     state
//         .catch_exception
//         .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
//         .expect("nested exception catching is not supported");

//     let do_func_result = do_func();
//     let result = state.exception.get_mut().take().map_or(Ok(do_func_result), Err);

//     state
//         .catch_exception
//         .compare_exchange(true, false, Ordering::Relaxed, Ordering::Relaxed)
//         .expect("inconsistent local catch state");

//     result
// }
