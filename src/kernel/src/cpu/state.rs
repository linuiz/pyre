use crate::{
    interrupts::{exceptions::Exception, new_controller, Controller, InterruptCell},
    task::Scheduler,
};
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
    int_ctrl: Box<dyn Controller>,
    scheduler: InterruptCell<Scheduler>,

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
pub unsafe fn init(timer_frequency: core::num::NonZeroU16) {
    let mut int_ctrl = new_controller();
    int_ctrl.set_timer_frequency(timer_frequency);
    let scheduler = InterruptCell::new(Scheduler::new(false));

    let state = Box::new(State {
        core_id: crate::cpu::read_id(),
        int_ctrl,
        scheduler,

        timer_interval: None,

        catch_exception: AtomicBool::new(false),
        exception: UnsafeCell::new(None),
    });

    // Safety: Local state init requires setting the kernel thread pointer.
    unsafe {
        crate::cpu::set_kernel_thread_ptr(Box::into_raw(state));
    }
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

    get_state_mut()?.int_ctrl.enable_timer();

    // Safety: Calling `begin_scheduling` implies this function is expected to be called, and its invariants are met.
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
    get_state_mut()?.int_ctrl.end_interrupt();

    Ok(())
}

/// ### Safety
///
/// Caller must ensure that setting a new preemption wait will not cause undefined behaviour.
pub unsafe fn set_preemption_wait(wait_factor: core::num::NonZeroU16) -> Result<()> {
    get_state_mut()?.int_ctrl.set_timer_wait(wait_factor);

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
