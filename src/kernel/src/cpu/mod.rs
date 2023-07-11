pub mod state;

pub fn read_id() -> u32 {
    #[cfg(target_arch = "x86_64")]
    {
        crate::arch::x86_64::get_cpu_id()
    }
}

pub unsafe fn set_kernel_thread_ptr<T>(ptr: *mut T) {
    #[cfg(target_arch = "x86_64")]
    crate::arch::x86_64::registers::msr::IA32_KERNEL_GS_BASE::write(ptr.addr() as u64);
}
