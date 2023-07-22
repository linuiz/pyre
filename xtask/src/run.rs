use anyhow::{Context, Result};
use clap::ValueEnum;
use xshell::cmd;

#[derive(ValueEnum, Clone, Copy, PartialEq, Eq)]
pub enum Accelerator {
    Kvm,
    None,
}

impl core::fmt::Debug for Accelerator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Accelerator::Kvm => "q35,accel=kvm",
            Accelerator::None => "q35",
        })
    }
}

#[derive(ValueEnum, Debug, Clone, Copy)]
pub enum Cpu {
    Host,
    Max,
    Qemu64,
    Rv64,
}

impl Cpu {
    pub const fn as_str(&self) -> &str {
        match self {
            Cpu::Host => "host",
            Cpu::Max => "max",
            Cpu::Qemu64 => "qemu64",
            Cpu::Rv64 => "rv64",
        }
    }
}

#[derive(ValueEnum, Clone, Copy)]
pub enum BlockDriver {
    Ahci,
    Nvme,
    Virtio,
}

impl core::fmt::Debug for BlockDriver {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            BlockDriver::Ahci => "ahci",
            BlockDriver::Nvme => "nvme",
            BlockDriver::Virtio => "virtio-blk-pci",
        })
    }
}

#[derive(clap::Parser)]
#[group(skip)]
pub struct Options {
    /// CPU type to emulate.
    #[arg(value_enum, long, default_value = "qemu64")]
    cpu: Cpu,

    /// Emulation accelerator to use.
    #[arg(value_enum, long, default_value = "none")]
    accel: Accelerator,

    /// Number of CPUs to emulate.
    #[arg(long, default_value = "4")]
    smp: usize,

    // RAM size in MB.
    #[arg(long, default_value = "512")]
    ram: usize,

    /// Enables debug logging to the specified location.
    #[arg(long)]
    log: bool,

    /// Which type of block driver to use for root drive.
    #[arg(value_enum, long, default_value = "virtio")]
    block: BlockDriver,

    /// Skips invoking the build pipeline for the kernel.
    #[arg(long)]
    nobuild: bool,

    /// Runs the kernel in serial-only mode (no graphics driving).
    #[arg(long)]
    nographics: bool,

    #[clap(flatten)]
    build_options: crate::build::Options,

    /// Skips execution and only prints the QEMU command that would have been executed.
    #[arg(short, long)]
    mock: bool,

    /// Puts QEMU in GDB debug mode, awaiting signal from the debugger to begin execution.
    #[arg(short, long)]
    gdb: bool,
}

pub fn run(sh: &xshell::Shell, options: Options) -> Result<()> {
    if !options.nobuild {
        crate::build::build(sh, options.build_options)?;
    }

    let qemu_exe_str = match options.cpu {
        Cpu::Rv64 => "qemu-system-riscv64",
        _ => "qemu-system-x86_64",
    };

    let options_machine = match (options.cpu, options.accel) {
        (Cpu::Rv64, Accelerator::None) => "virt",
        (Cpu::Rv64, accel) => panic!("invalid accelerator for RISC-V: {:?}", accel),

        (_, Accelerator::Kvm) => "q35,accel=kvm",
        (_, Accelerator::None) => "q35",
    };

    let options_smp_owned = options.smp.to_string();
    let options_smp = options_smp_owned.as_str();

    let options_cpu = options.cpu.as_str();

    let options_ram_owned = format!("{}M", options.ram);
    let options_ram = options_ram_owned.as_str();

    let options_device_owned = format!("{:?},drive=disk1,serial=deadbeef", options.block);
    let options_device = options_device_owned.as_str();

    let mut optional_args = vec![];

    if options.log {
        optional_args.extend_from_slice(&["-d", "int,guest_errors", "-D", ".debug/qemu.log"]);
    }

    match options.cpu {
        Cpu::Rv64 => unimplemented!(),
        _ => optional_args.extend_from_slice(&[
            "-drive",
            "if=pflash,index=0,readonly=on,format=raw,file=build/ovmf/x86_64/code.fd",
            "-drive",
            "if=pflash,index=1,format=raw,file=build/ovmf/x86_64/vars.fd",
            "-drive",
            "format=raw,file=fat:rw:build/root/",
        ]),
    };

    if options.nographics {
        optional_args.push("-nographic");
    }

    if options.gdb {
        optional_args.extend_from_slice(&["-S", "-s"]);
    }

    let cmd = cmd!(
        sh,
        "
        {qemu_exe_str}
            -no-shutdown
            -no-reboot
            -serial mon:stdio
            -drive format=raw,file=build/disk0.img,id=disk1,if=none
            -net none
            -M smm=off
            -machine {options_machine}
            -cpu {options_cpu}
            -smp {options_smp}
            -m {options_ram}
            -device {options_device}
            {optional_args...}
        "
    );

    if options.mock {
        println!("cmd: {}", cmd.to_string());
    } else {
        cmd.run()?;
    }

    Ok(())
}
