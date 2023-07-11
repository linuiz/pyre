use crate::{
    interrupts::exceptions::{ex_handler, ArchException},
    task::{Registers, State},
};
use ia32utils::structures::idt::*;
use libsys::Address;

/// Loads the IDT from the interrupt descriptor table register.
pub unsafe fn get_current() -> Option<&'static mut InterruptDescriptorTable> {
    let idt_pointer = ia32utils::instructions::tables::sidt();
    idt_pointer.base.as_mut_ptr::<InterruptDescriptorTable>().as_mut()
}

macro_rules! push_gprs {
    () => {
        "
        push r15
        push r14
        push r13
        push r12
        push r11
        push r10
        push r9
        push r8
        push rbp
        push rsi
        push rdi
        push rdx
        push rcx
        push rbx
        push rax
        "
    };
}

macro_rules! pop_gprs {
    () => {
        "
        pop rax
        pop rbx
        pop rcx
        pop rdx
        pop rdi
        pop rsi
        pop rbp
        pop r8
        pop r9
        pop r10
        pop r11
        pop r12
        pop r13
        pop r14
        pop r15
        "
    };
}

macro_rules! push_ret_frame {
    ($ip_off:literal) => {
        concat!(
            "
            # copy code segment to `rax`
            mov rax, [rsp + (",
            stringify!($ip_off + 1),
            " * 8)]

            # We don't want to try and trace a fault in the kernel back to
            # userspace, so we check if we're coming from the kernel.
            cmp rax, 0x8    # are we coming from kernel code?
            je 2f           # if so, don't zero the frame pointer
            xor rbp, rbp    # if not, zero the frame pointer
            2:

            # copy ip to `rax`
            mov rax, [rsp + (",
            stringify!($ip_off),
            " * 8)]

            # push return frame
            push rax                # push instruction pointer
            push rbp                # push frame pointer
            mov rbp, rsp
            "
        )
    };
}

macro_rules! exception_handler {
    ($exception_name:ident, $return_type:ty) => {
        paste::paste! {
            #[naked]
            pub extern "x86-interrupt" fn [<$exception_name _handler>](stack_frame: InterruptStackFrame) -> $return_type {
                // Safety: When has perfect assembly ever caused undefined behaviour?
                unsafe {
                    core::arch::asm!(
                        "cld",
                        push_gprs!(),
                        push_ret_frame!(15),
                        "
                        # move stack frame into first parameter
                        lea rdi, [rsp + (17 * 8)]
                        # Move cached gprs pointer into second parameter.
                        lea rsi, [rsp + (2 * 8)]

                        call {}

                        add rsp, 0x10   # 'pop' stack frame
                        ", pop_gprs!(), "

                        iretq
                        ",
                        sym [<$exception_name _handler_inner>],
                        options(noreturn)
                    )
                }
            }
        }
    };
}

macro_rules! exception_handler_with_error {
    ($exception_name:ident, $error_ty:ty, $return_type:ty) => {
        paste::paste! {
            #[naked]
            pub extern "x86-interrupt" fn [<$exception_name _handler>](
                stack_frame: InterruptStackFrame,
                error_code: $error_ty
            ) -> $return_type {
                // Safety: When has perfect assembly ever caused undefined behaviour?
                unsafe {
                    core::arch::asm!(
                        "cld",
                        push_gprs!(),
                        push_ret_frame!(16),
                        "
                        # Move stack frame into first parameter.
                        lea rdi, [rsp + (18 * 8)]
                        # Move error code into second parameter.
                        mov rsi, [rsp + (17 * 8)]
                        # Move cached gprs pointer into third parameter.
                        lea rdx, [rsp + (2 * 8)]

                        # align stack for SysV
                        sub rsp, 0x8

                        call {}

                        add rsp, 0x18   # 'pop' sysv fn-align & stack frame
                        ", pop_gprs!(), "
                        add rsp, 0x8    # 'pop' error code

                        iretq
                        ",
                        sym [<$exception_name _handler_inner>],
                        options(noreturn)
                    )
                }
            }
        }
    };
}

macro_rules! irq_stub {
    ($irq_vector:literal) => {
        paste::paste! {
            #[naked]
            pub extern "x86-interrupt" fn [<irq_ $irq_vector>](_: ia32utils::structures::idt::InterruptStackFrame) {
                // Safety: This is literally perfect assembly. It's safe because it's perfect.
                unsafe {
                    core::arch::asm!(
                        "cld",
                        push_gprs!(),
                        push_ret_frame!(15),
                        "

                        # Move IRQ vector into first parameter.
                        mov rdi, {}
                        # Move stack frame into second parameter.
                        lea rsi, [rsp + (17 * 8)]
                        # Move cached gprs pointer into third parameter.
                        lea rdx, [rsp + (2 * 8)]

                        call {}

                        add rsp, 0x10   # 'pop' stack frame
                        ", pop_gprs!(), "

                        iretq
                        ",
                        const $irq_vector,
                        sym irq_handoff,
                        options(noreturn)
                    );
                }
            }
        }
    };
}

/// ### Safety
///
/// This function should not be called from software.
#[allow(clippy::cast_possible_truncation)]
unsafe extern "sysv64" fn irq_handoff(irq_number: u64, isf: &mut InterruptStackFrame, regs: &mut Registers) {
    use crate::arch::x86_64::registers::RFlags;
    use ia32utils::VirtAddr;

    let mut state = State {
        ip: Address::from_ptr(isf.instruction_pointer.as_mut_ptr::<()>()),
        cs: usize::try_from(isf.code_segment).unwrap(),
        rfl: RFlags::from_bits_retain(isf.cpu_flags as usize),
        sp: Address::from_ptr(isf.stack_pointer.as_mut_ptr::<()>()),
        ss: usize::try_from(isf.stack_segment).unwrap(),
    };

    crate::interrupts::traps::handle_trap(irq_number, &mut state, regs);

    isf.as_mut().write(InterruptStackFrameValue {
        instruction_pointer: VirtAddr::from_ptr(state.ip.as_ptr()),
        code_segment: u64::try_from(state.cs).unwrap(),
        cpu_flags: u64::try_from(state.rfl.bits()).unwrap(),
        stack_pointer: VirtAddr::from_ptr(state.sp.as_ptr()),
        stack_segment: u64::try_from(state.ss).unwrap(),
    });
}

exception_handler!(de, ());
extern "sysv64" fn de_handler_inner(stack_frame: &InterruptStackFrame, gprs: &Registers) {
    ex_handler(&ArchException::DivideError(stack_frame, gprs));
}

exception_handler!(db, ());
extern "sysv64" fn db_handler_inner(stack_frame: &InterruptStackFrame, gprs: &Registers) {
    ex_handler(&ArchException::Debug(stack_frame, gprs));
}

exception_handler!(nmi, ());
extern "sysv64" fn nmi_handler_inner(stack_frame: &InterruptStackFrame, gprs: &Registers) {
    ex_handler(&ArchException::NonMaskable(stack_frame, gprs));
}

exception_handler!(bp, ());
extern "sysv64" fn bp_handler_inner(stack_frame: &InterruptStackFrame, gprs: &Registers) {
    ex_handler(&ArchException::Breakpoint(stack_frame, gprs));
}

exception_handler!(of, ());
extern "sysv64" fn of_handler_inner(stack_frame: &InterruptStackFrame, gprs: &Registers) {
    ex_handler(&ArchException::Overflow(stack_frame, gprs));
}

exception_handler!(br, ());
extern "sysv64" fn br_handler_inner(stack_frame: &InterruptStackFrame, gprs: &Registers) {
    ex_handler(&ArchException::BoundRangeExceeded(stack_frame, gprs));
}

exception_handler!(ud, ());
extern "sysv64" fn ud_handler_inner(stack_frame: &InterruptStackFrame, gprs: &Registers) {
    ex_handler(&ArchException::InvalidOpcode(stack_frame, gprs));
}

exception_handler!(nm, ());
extern "sysv64" fn nm_handler_inner(stack_frame: &InterruptStackFrame, gprs: &Registers) {
    ex_handler(&ArchException::DeviceNotAvailable(stack_frame, gprs));
}

exception_handler_with_error!(df, u64, !);
extern "sysv64" fn df_handler_inner(stack_frame: &InterruptStackFrame, _: u64, gprs: &Registers) -> ! {
    ex_handler(&ArchException::DoubleFault(stack_frame, gprs));

    unreachable!()
}

exception_handler_with_error!(ts, u64, ());
extern "sysv64" fn ts_handler_inner(stack_frame: &InterruptStackFrame, error_code: u64, gprs: &Registers) {
    ex_handler(&ArchException::InvalidTSS(stack_frame, SelectorErrorCode::new_truncate(error_code), gprs));
}

exception_handler_with_error!(np, u64, ());
extern "sysv64" fn np_handler_inner(stack_frame: &InterruptStackFrame, error_code: u64, gprs: &Registers) {
    ex_handler(&ArchException::SegmentNotPresent(stack_frame, SelectorErrorCode::new_truncate(error_code), gprs));
}

exception_handler_with_error!(ss, u64, ());
extern "sysv64" fn ss_handler_inner(stack_frame: &InterruptStackFrame, error_code: u64, gprs: &Registers) {
    ex_handler(&ArchException::StackSegmentFault(stack_frame, SelectorErrorCode::new_truncate(error_code), gprs));
}

exception_handler_with_error!(gp, u64, ());
extern "sysv64" fn gp_handler_inner(stack_frame: &InterruptStackFrame, error_code: u64, gprs: &Registers) {
    ex_handler(&ArchException::GeneralProtectionFault(stack_frame, SelectorErrorCode::new_truncate(error_code), gprs));
}

exception_handler_with_error!(pf, PageFaultErrorCode, ());
extern "sysv64" fn pf_handler_inner(stack_frame: &InterruptStackFrame, err: PageFaultErrorCode, gprs: &Registers) {
    ex_handler(&ArchException::PageFault(stack_frame, gprs, err, crate::arch::x86_64::registers::control::CR2::read()));
}

// --- reserved 15

exception_handler!(mf, ());
extern "sysv64" fn mf_handler_inner(stack_frame: &InterruptStackFrame, gprs: &Registers) {
    ex_handler(&ArchException::x87FloatingPoint(stack_frame, gprs));
}

exception_handler_with_error!(ac, u64, ());
extern "sysv64" fn ac_handler_inner(stack_frame: &InterruptStackFrame, error_code: u64, gprs: &Registers) {
    ex_handler(&ArchException::AlignmentCheck(stack_frame, error_code, gprs));
}

exception_handler!(mc, !);
extern "sysv64" fn mc_handler_inner(stack_frame: &InterruptStackFrame, gprs: &Registers) -> ! {
    ex_handler(&ArchException::MachineCheck(stack_frame, gprs));
    // Wait indefinite in case the above exception handler returns control flow.
    crate::interrupts::wait_loop()
}

exception_handler!(xm, ());
extern "sysv64" fn xm_handler_inner(stack_frame: &InterruptStackFrame, gprs: &Registers) {
    ex_handler(&ArchException::SimdFlaotingPoint(stack_frame, gprs));
}

exception_handler!(ve, ());
extern "sysv64" fn ve_handler_inner(stack_frame: &InterruptStackFrame, gprs: &Registers) {
    ex_handler(&ArchException::Virtualization(stack_frame, gprs));
}

// --- reserved 22-30

// --- triple fault (can't handle)

pub fn set_exception_handlers(idt: &mut InterruptDescriptorTable) {
    use super::StackTableIndex;

    // Safety: These are nominal and agreed-upon states for the given exception vectors.
    unsafe {
        idt.debug.set_handler_fn(db_handler).set_stack_index(StackTableIndex::Debug as u16);
        idt.non_maskable_interrupt.set_handler_fn(nmi_handler).set_stack_index(StackTableIndex::NonMaskable as u16);
        idt.double_fault.set_handler_fn(df_handler).set_stack_index(StackTableIndex::DoubleFault as u16);
        idt.machine_check.set_handler_fn(mc_handler).set_stack_index(StackTableIndex::MachineCheck as u16);
    }

    idt.divide_error.set_handler_fn(de_handler);
    idt.breakpoint.set_handler_fn(bp_handler);
    idt.overflow.set_handler_fn(of_handler);
    idt.bound_range_exceeded.set_handler_fn(br_handler);
    idt.invalid_opcode.set_handler_fn(ud_handler);
    idt.device_not_available.set_handler_fn(nm_handler);
    idt.invalid_tss.set_handler_fn(ts_handler);
    idt.segment_not_present.set_handler_fn(np_handler);
    idt.stack_segment_fault.set_handler_fn(ss_handler);
    idt.general_protection_fault.set_handler_fn(gp_handler);
    idt.page_fault.set_handler_fn(pf_handler);
    // --- reserved 15
    idt.x87_floating_point.set_handler_fn(mf_handler);
    idt.alignment_check.set_handler_fn(ac_handler);
    idt.simd_floating_point.set_handler_fn(xm_handler);
    idt.virtualization.set_handler_fn(ve_handler);
    // --- reserved 20-31
    // --- triple fault (can't handle)
}

#[allow(clippy::too_many_lines)]
pub fn set_stub_handlers(idt: &mut InterruptDescriptorTable) {
    // userspace syscall vector
    idt[128].set_handler_fn(irq_128).set_privilege_level(ia32utils::PrivilegeLevel::Ring3);

    idt[32].set_handler_fn(irq_32);
    idt[33].set_handler_fn(irq_33);
    idt[34].set_handler_fn(irq_34);
    idt[35].set_handler_fn(irq_35);
    idt[36].set_handler_fn(irq_36);
    idt[37].set_handler_fn(irq_37);
    idt[38].set_handler_fn(irq_38);
    idt[39].set_handler_fn(irq_39);
    idt[40].set_handler_fn(irq_40);
    idt[41].set_handler_fn(irq_41);
    idt[42].set_handler_fn(irq_42);
    idt[43].set_handler_fn(irq_43);
    idt[44].set_handler_fn(irq_44);
    idt[45].set_handler_fn(irq_45);
    idt[46].set_handler_fn(irq_46);
    idt[47].set_handler_fn(irq_47);
    idt[48].set_handler_fn(irq_48);
    idt[49].set_handler_fn(irq_49);
    idt[50].set_handler_fn(irq_50);
    idt[51].set_handler_fn(irq_51);
    idt[52].set_handler_fn(irq_52);
    idt[53].set_handler_fn(irq_53);
    idt[54].set_handler_fn(irq_54);
    idt[55].set_handler_fn(irq_55);
    idt[56].set_handler_fn(irq_56);
    idt[57].set_handler_fn(irq_57);
    idt[58].set_handler_fn(irq_58);
    idt[59].set_handler_fn(irq_59);
    idt[60].set_handler_fn(irq_60);
    idt[61].set_handler_fn(irq_61);
    idt[62].set_handler_fn(irq_62);
    idt[63].set_handler_fn(irq_63);
    idt[64].set_handler_fn(irq_64);
    idt[65].set_handler_fn(irq_65);
    idt[66].set_handler_fn(irq_66);
    idt[67].set_handler_fn(irq_67);
    idt[68].set_handler_fn(irq_68);
    idt[69].set_handler_fn(irq_69);
    idt[70].set_handler_fn(irq_70);
    idt[71].set_handler_fn(irq_71);
    idt[72].set_handler_fn(irq_72);
    idt[73].set_handler_fn(irq_73);
    idt[74].set_handler_fn(irq_74);
    idt[75].set_handler_fn(irq_75);
    idt[76].set_handler_fn(irq_76);
    idt[77].set_handler_fn(irq_77);
    idt[78].set_handler_fn(irq_78);
    idt[79].set_handler_fn(irq_79);
    idt[80].set_handler_fn(irq_80);
    idt[81].set_handler_fn(irq_81);
    idt[82].set_handler_fn(irq_82);
    idt[83].set_handler_fn(irq_83);
    idt[84].set_handler_fn(irq_84);
    idt[85].set_handler_fn(irq_85);
    idt[86].set_handler_fn(irq_86);
    idt[87].set_handler_fn(irq_87);
    idt[88].set_handler_fn(irq_88);
    idt[89].set_handler_fn(irq_89);
    idt[90].set_handler_fn(irq_90);
    idt[91].set_handler_fn(irq_91);
    idt[92].set_handler_fn(irq_92);
    idt[93].set_handler_fn(irq_93);
    idt[94].set_handler_fn(irq_94);
    idt[95].set_handler_fn(irq_95);
    idt[96].set_handler_fn(irq_96);
    idt[97].set_handler_fn(irq_97);
    idt[98].set_handler_fn(irq_98);
    idt[99].set_handler_fn(irq_99);
    idt[100].set_handler_fn(irq_100);
    idt[101].set_handler_fn(irq_101);
    idt[102].set_handler_fn(irq_102);
    idt[103].set_handler_fn(irq_103);
    idt[104].set_handler_fn(irq_104);
    idt[105].set_handler_fn(irq_105);
    idt[106].set_handler_fn(irq_106);
    idt[107].set_handler_fn(irq_107);
    idt[108].set_handler_fn(irq_108);
    idt[109].set_handler_fn(irq_109);
    idt[110].set_handler_fn(irq_110);
    idt[111].set_handler_fn(irq_111);
    idt[112].set_handler_fn(irq_112);
    idt[113].set_handler_fn(irq_113);
    idt[114].set_handler_fn(irq_114);
    idt[115].set_handler_fn(irq_115);
    idt[116].set_handler_fn(irq_116);
    idt[117].set_handler_fn(irq_117);
    idt[118].set_handler_fn(irq_118);
    idt[119].set_handler_fn(irq_119);
    idt[120].set_handler_fn(irq_120);
    idt[121].set_handler_fn(irq_121);
    idt[122].set_handler_fn(irq_122);
    idt[123].set_handler_fn(irq_123);
    idt[124].set_handler_fn(irq_124);
    idt[125].set_handler_fn(irq_125);
    idt[126].set_handler_fn(irq_126);
    idt[127].set_handler_fn(irq_127);
    idt[129].set_handler_fn(irq_129);
    idt[130].set_handler_fn(irq_130);
    idt[131].set_handler_fn(irq_131);
    idt[132].set_handler_fn(irq_132);
    idt[133].set_handler_fn(irq_133);
    idt[134].set_handler_fn(irq_134);
    idt[135].set_handler_fn(irq_135);
    idt[136].set_handler_fn(irq_136);
    idt[137].set_handler_fn(irq_137);
    idt[138].set_handler_fn(irq_138);
    idt[139].set_handler_fn(irq_139);
    idt[140].set_handler_fn(irq_140);
    idt[141].set_handler_fn(irq_141);
    idt[142].set_handler_fn(irq_142);
    idt[143].set_handler_fn(irq_143);
    idt[144].set_handler_fn(irq_144);
    idt[145].set_handler_fn(irq_145);
    idt[146].set_handler_fn(irq_146);
    idt[147].set_handler_fn(irq_147);
    idt[148].set_handler_fn(irq_148);
    idt[149].set_handler_fn(irq_149);
    idt[150].set_handler_fn(irq_150);
    idt[151].set_handler_fn(irq_151);
    idt[152].set_handler_fn(irq_152);
    idt[153].set_handler_fn(irq_153);
    idt[154].set_handler_fn(irq_154);
    idt[155].set_handler_fn(irq_155);
    idt[156].set_handler_fn(irq_156);
    idt[157].set_handler_fn(irq_157);
    idt[158].set_handler_fn(irq_158);
    idt[159].set_handler_fn(irq_159);
    idt[160].set_handler_fn(irq_160);
    idt[161].set_handler_fn(irq_161);
    idt[162].set_handler_fn(irq_162);
    idt[163].set_handler_fn(irq_163);
    idt[164].set_handler_fn(irq_164);
    idt[165].set_handler_fn(irq_165);
    idt[166].set_handler_fn(irq_166);
    idt[167].set_handler_fn(irq_167);
    idt[168].set_handler_fn(irq_168);
    idt[169].set_handler_fn(irq_169);
    idt[170].set_handler_fn(irq_170);
    idt[171].set_handler_fn(irq_171);
    idt[172].set_handler_fn(irq_172);
    idt[173].set_handler_fn(irq_173);
    idt[174].set_handler_fn(irq_174);
    idt[175].set_handler_fn(irq_175);
    idt[176].set_handler_fn(irq_176);
    idt[177].set_handler_fn(irq_177);
    idt[178].set_handler_fn(irq_178);
    idt[179].set_handler_fn(irq_179);
    idt[180].set_handler_fn(irq_180);
    idt[181].set_handler_fn(irq_181);
    idt[182].set_handler_fn(irq_182);
    idt[183].set_handler_fn(irq_183);
    idt[184].set_handler_fn(irq_184);
    idt[185].set_handler_fn(irq_185);
    idt[186].set_handler_fn(irq_186);
    idt[187].set_handler_fn(irq_187);
    idt[188].set_handler_fn(irq_188);
    idt[189].set_handler_fn(irq_189);
    idt[190].set_handler_fn(irq_190);
    idt[191].set_handler_fn(irq_191);
    idt[192].set_handler_fn(irq_192);
    idt[193].set_handler_fn(irq_193);
    idt[194].set_handler_fn(irq_194);
    idt[195].set_handler_fn(irq_195);
    idt[196].set_handler_fn(irq_196);
    idt[197].set_handler_fn(irq_197);
    idt[198].set_handler_fn(irq_198);
    idt[199].set_handler_fn(irq_199);
    idt[200].set_handler_fn(irq_200);
    idt[201].set_handler_fn(irq_201);
    idt[202].set_handler_fn(irq_202);
    idt[203].set_handler_fn(irq_203);
    idt[204].set_handler_fn(irq_204);
    idt[205].set_handler_fn(irq_205);
    idt[206].set_handler_fn(irq_206);
    idt[207].set_handler_fn(irq_207);
    idt[208].set_handler_fn(irq_208);
    idt[209].set_handler_fn(irq_209);
    idt[210].set_handler_fn(irq_210);
    idt[211].set_handler_fn(irq_211);
    idt[212].set_handler_fn(irq_212);
    idt[213].set_handler_fn(irq_213);
    idt[214].set_handler_fn(irq_214);
    idt[215].set_handler_fn(irq_215);
    idt[216].set_handler_fn(irq_216);
    idt[217].set_handler_fn(irq_217);
    idt[218].set_handler_fn(irq_218);
    idt[219].set_handler_fn(irq_219);
    idt[220].set_handler_fn(irq_220);
    idt[221].set_handler_fn(irq_221);
    idt[222].set_handler_fn(irq_222);
    idt[223].set_handler_fn(irq_223);
    idt[224].set_handler_fn(irq_224);
    idt[225].set_handler_fn(irq_225);
    idt[226].set_handler_fn(irq_226);
    idt[227].set_handler_fn(irq_227);
    idt[228].set_handler_fn(irq_228);
    idt[229].set_handler_fn(irq_229);
    idt[230].set_handler_fn(irq_230);
    idt[231].set_handler_fn(irq_231);
    idt[232].set_handler_fn(irq_232);
    idt[233].set_handler_fn(irq_233);
    idt[234].set_handler_fn(irq_234);
    idt[235].set_handler_fn(irq_235);
    idt[236].set_handler_fn(irq_236);
    idt[237].set_handler_fn(irq_237);
    idt[238].set_handler_fn(irq_238);
    idt[239].set_handler_fn(irq_239);
    idt[240].set_handler_fn(irq_240);
    idt[241].set_handler_fn(irq_241);
    idt[242].set_handler_fn(irq_242);
    idt[243].set_handler_fn(irq_243);
    idt[244].set_handler_fn(irq_244);
    idt[245].set_handler_fn(irq_245);
    idt[246].set_handler_fn(irq_246);
    idt[247].set_handler_fn(irq_247);
    idt[248].set_handler_fn(irq_248);
    idt[249].set_handler_fn(irq_249);
    idt[250].set_handler_fn(irq_250);
    idt[251].set_handler_fn(irq_251);
    idt[252].set_handler_fn(irq_252);
    idt[253].set_handler_fn(irq_253);
    idt[254].set_handler_fn(irq_254);
    idt[255].set_handler_fn(irq_255);
}

irq_stub!(32);
irq_stub!(33);
irq_stub!(34);
irq_stub!(35);
irq_stub!(36);
irq_stub!(37);
irq_stub!(38);
irq_stub!(39);
irq_stub!(40);
irq_stub!(41);
irq_stub!(42);
irq_stub!(43);
irq_stub!(44);
irq_stub!(45);
irq_stub!(46);
irq_stub!(47);
irq_stub!(48);
irq_stub!(49);
irq_stub!(50);
irq_stub!(51);
irq_stub!(52);
irq_stub!(53);
irq_stub!(54);
irq_stub!(55);
irq_stub!(56);
irq_stub!(57);
irq_stub!(58);
irq_stub!(59);
irq_stub!(60);
irq_stub!(61);
irq_stub!(62);
irq_stub!(63);
irq_stub!(64);
irq_stub!(65);
irq_stub!(66);
irq_stub!(67);
irq_stub!(68);
irq_stub!(69);
irq_stub!(70);
irq_stub!(71);
irq_stub!(72);
irq_stub!(73);
irq_stub!(74);
irq_stub!(75);
irq_stub!(76);
irq_stub!(77);
irq_stub!(78);
irq_stub!(79);
irq_stub!(80);
irq_stub!(81);
irq_stub!(82);
irq_stub!(83);
irq_stub!(84);
irq_stub!(85);
irq_stub!(86);
irq_stub!(87);
irq_stub!(88);
irq_stub!(89);
irq_stub!(90);
irq_stub!(91);
irq_stub!(92);
irq_stub!(93);
irq_stub!(94);
irq_stub!(95);
irq_stub!(96);
irq_stub!(97);
irq_stub!(98);
irq_stub!(99);
irq_stub!(100);
irq_stub!(101);
irq_stub!(102);
irq_stub!(103);
irq_stub!(104);
irq_stub!(105);
irq_stub!(106);
irq_stub!(107);
irq_stub!(108);
irq_stub!(109);
irq_stub!(110);
irq_stub!(111);
irq_stub!(112);
irq_stub!(113);
irq_stub!(114);
irq_stub!(115);
irq_stub!(116);
irq_stub!(117);
irq_stub!(118);
irq_stub!(119);
irq_stub!(120);
irq_stub!(121);
irq_stub!(122);
irq_stub!(123);
irq_stub!(124);
irq_stub!(125);
irq_stub!(126);
irq_stub!(127);
irq_stub!(128);
irq_stub!(129);
irq_stub!(130);
irq_stub!(131);
irq_stub!(132);
irq_stub!(133);
irq_stub!(134);
irq_stub!(135);
irq_stub!(136);
irq_stub!(137);
irq_stub!(138);
irq_stub!(139);
irq_stub!(140);
irq_stub!(141);
irq_stub!(142);
irq_stub!(143);
irq_stub!(144);
irq_stub!(145);
irq_stub!(146);
irq_stub!(147);
irq_stub!(148);
irq_stub!(149);
irq_stub!(150);
irq_stub!(151);
irq_stub!(152);
irq_stub!(153);
irq_stub!(154);
irq_stub!(155);
irq_stub!(156);
irq_stub!(157);
irq_stub!(158);
irq_stub!(159);
irq_stub!(160);
irq_stub!(161);
irq_stub!(162);
irq_stub!(163);
irq_stub!(164);
irq_stub!(165);
irq_stub!(166);
irq_stub!(167);
irq_stub!(168);
irq_stub!(169);
irq_stub!(170);
irq_stub!(171);
irq_stub!(172);
irq_stub!(173);
irq_stub!(174);
irq_stub!(175);
irq_stub!(176);
irq_stub!(177);
irq_stub!(178);
irq_stub!(179);
irq_stub!(180);
irq_stub!(181);
irq_stub!(182);
irq_stub!(183);
irq_stub!(184);
irq_stub!(185);
irq_stub!(186);
irq_stub!(187);
irq_stub!(188);
irq_stub!(189);
irq_stub!(190);
irq_stub!(191);
irq_stub!(192);
irq_stub!(193);
irq_stub!(194);
irq_stub!(195);
irq_stub!(196);
irq_stub!(197);
irq_stub!(198);
irq_stub!(199);
irq_stub!(200);
irq_stub!(201);
irq_stub!(202);
irq_stub!(203);
irq_stub!(204);
irq_stub!(205);
irq_stub!(206);
irq_stub!(207);
irq_stub!(208);
irq_stub!(209);
irq_stub!(210);
irq_stub!(211);
irq_stub!(212);
irq_stub!(213);
irq_stub!(214);
irq_stub!(215);
irq_stub!(216);
irq_stub!(217);
irq_stub!(218);
irq_stub!(219);
irq_stub!(220);
irq_stub!(221);
irq_stub!(222);
irq_stub!(223);
irq_stub!(224);
irq_stub!(225);
irq_stub!(226);
irq_stub!(227);
irq_stub!(228);
irq_stub!(229);
irq_stub!(230);
irq_stub!(231);
irq_stub!(232);
irq_stub!(233);
irq_stub!(234);
irq_stub!(235);
irq_stub!(236);
irq_stub!(237);
irq_stub!(238);
irq_stub!(239);
irq_stub!(240);
irq_stub!(241);
irq_stub!(242);
irq_stub!(243);
irq_stub!(244);
irq_stub!(245);
irq_stub!(246);
irq_stub!(247);
irq_stub!(248);
irq_stub!(249);
irq_stub!(250);
irq_stub!(251);
irq_stub!(252);
irq_stub!(253);
irq_stub!(254);
irq_stub!(255);
