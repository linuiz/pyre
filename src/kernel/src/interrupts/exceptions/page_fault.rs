use libsys::{Address, Virtual};

errorgen! {
    /// Indicates what type of error the common page fault handler encountered.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Error {
        CoreState => None,
        NoTask => None,
        Task { err: crate::task::Error } => Some(err),
    }
}

/// ### Safety
///
/// This function should only be called in the case of passing context to handle a page fault.
/// Calling this function more than once and/or outside the context of a page fault is undefined behaviour.
#[doc(hidden)]
#[inline(never)]
pub unsafe fn handler(fault_address: Address<Virtual>) -> Result<()> {
    crate::cpu::state::with_scheduler(|scheduler| {
        scheduler.task_mut().ok_or(Error::NoTask)?.demand_map(fault_address).map_err(|err| Error::Task { err })
    })?;

    Ok(())
}
