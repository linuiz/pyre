use crate::{
    mem::StackUnit,
    task::{Registers, State, Task},
};
use alloc::collections::VecDeque;
use libsys::Address;

pub static PROCESSES: spin::Mutex<VecDeque<Task>> = spin::Mutex::new(VecDeque::new());

pub struct Scheduler {
    enabled: bool,
    idle_stack: [StackUnit; 0x1000],
    task: Option<Task>,
}

impl Scheduler {
    pub const fn new(enabled: bool) -> Self {
        Self { enabled, idle_stack: [StackUnit(0); 0x1000], task: None }
    }

    /// Enables the scheduler to pop tasks.
    #[inline]
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disables scheduler from popping tasks. Any task pops which are already in-flight will not be cancelled.
    #[inline]
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Indicates whether the scheduler is enabled.
    #[inline]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    #[inline]
    pub const fn process(&self) -> Option<&Task> {
        self.task.as_ref()
    }

    #[inline]
    pub fn task_mut(&mut self) -> Option<&mut Task> {
        self.task.as_mut()
    }

    pub fn interrupt_task(&mut self, state: &mut State, regs: &mut Registers) {
        debug_assert!(!crate::interrupts::are_enabled());

        let mut processes = PROCESSES.lock();

        // Move the current task, if any, back into the scheduler queue.
        if let Some(mut process) = self.task.take() {
            trace!("Interrupting task: {:?}", process.id());

            process.context.0 = *state;
            process.context.1 = *regs;

            processes.push_back(process);
        }

        self.next_task(&mut processes, state, regs);
    }

    /// Attempts to schedule the next task in the local task queue.
    pub fn yield_task(&mut self, state: &mut State, regs: &mut Registers) {
        debug_assert!(!crate::interrupts::are_enabled());

        let mut processes = PROCESSES.lock();

        let mut process = self.task.take().expect("cannot yield without process");
        trace!("Yielding task: {:?}", process.id());

        process.context.0 = *state;
        process.context.1 = *regs;

        processes.push_back(process);

        self.next_task(&mut processes, state, regs);
    }

    pub fn kill_task(&mut self, state: &mut State, regs: &mut Registers) {
        debug_assert!(!crate::interrupts::are_enabled());

        // TODO add process to reap queue to reclaim address space memory
        let process = self.task.take().expect("cannot exit without process");
        trace!("Exiting process: {:?}", process.id());

        let mut processes = PROCESSES.lock();
        self.next_task(&mut processes, state, regs);
    }

    fn next_task(&mut self, processes: &mut VecDeque<Task>, state: &mut State, regs: &mut Registers) {
        // Pop a new task from the task queue, or simply switch in the idle task.
        if let Some(next_process) = processes.pop_front() {
            *state = next_process.context.0;
            *regs = next_process.context.1;

            if !next_process.address_space.is_current() {
                // Safety: New task requires its own address space.
                unsafe {
                    next_process.address_space.swap_into();
                }
            }

            trace!("Switched task: {:?}", next_process.id());
            let old_value = self.task.replace(next_process);
            debug_assert!(old_value.is_none());
        } else {
            *state = State::kernel(
                Address::new(crate::interrupts::wait_loop as usize).unwrap(),
                Address::from_ptr(self.idle_stack.as_ptr_range().end.cast_mut()),
            );
            *regs = Registers::default();

            trace!("Switched idle task.");
        };

        // TODO have some kind of queue of preemption waits, to ensure we select the shortest one.
        // Safety: Just having switched tasks, no preemption wait should supercede this one.
        unsafe {
            const TIME_SLICE: core::num::NonZeroU16 = core::num::NonZeroU16::new(5).unwrap();

            crate::cpu::state::set_preemption_wait(TIME_SLICE).unwrap();
        }
    }
}

// #[cfg(target_arch = "x86_64")]
// #[naked]
// unsafe extern "sysv64" fn exit_into(regs: &mut Registers, state: &mut State) -> ! {
//     use core::mem::size_of;
//     use x86_64::structures::idt::InterruptStackFrame;

//     core::arch::asm!(
//         "
//         mov rax, rdi    # registers ptr

//         sub rsp, {0}    # make space for stack frame
//         # state ptr is already in `rsi` from args
//         mov rdi, rsp    # dest is stack address
//         mov rcx, {0}    # set the copy length

//         cld             # clear direction for op
//         rep movsb       # copy memory

//         mov rbx, [rax + (1 * 8)]
//         mov rcx, [rax + (2 * 8)]
//         mov rdx, [rax + (3 * 8)]
//         mov rsi, [rax + (4 * 8)]
//         mov rdi, [rax + (5 * 8)]
//         mov rbp, [rax + (6 * 8)]
//         mov r8, [rax + (7 * 8)]
//         mov r9, [rax + (8 * 8)]
//         mov r10, [rax + (9 * 8)]
//         mov r11, [rax + (10 * 8)]
//         mov r12, [rax + (11 * 8)]
//         mov r13, [rax + (12 * 8)]
//         mov r14, [rax + (13 * 8)]
//         mov r15, [rax + (14 * 8)]
//         mov rax, [rax + (0 * 8)]

//         iretq
//         ",
//         const size_of::<InterruptStackFrame>(),
//         options(noreturn)
//     )
// }
