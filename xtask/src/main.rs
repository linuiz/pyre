mod build;
mod run;
mod target;

use anyhow::{Context, Result};
use clap::Parser;
use std::{
    fs::File,
    io::{copy, BufReader, Cursor},
    path::Path,
};
use xshell::{cmd, Shell, TempDir};

static WORKSPACE_DIRS: [&str; 3] = ["src/kernel", "src/shared", "src/userspace"];

static LIMINE_UEFI_IMAGE_URL: &str =
    "https://raw.githubusercontent.com/limine-bootloader/limine/v4.x-branch-binary/BOOTX64.EFI";
static LIMINE_DEFAULT_CFG: &str = r#"
TIMEOUT=3
SERIAL=yes

:Linuiz (limine)
COMMENT=Load Linuiz OS using the Limine boot protocol.
PROTOCOL=limine
RESOLUTION=800x600x16
KERNEL_PATH=boot:///linuiz/kernel
MODULE_PATH=boot:///linuiz/drivers
KASLR=yes
"#;

static UEFI_FIRMWARE_IMAGE_URL: &str = "https://github.com/rust-osdev/ovmf-prebuilt/releases/download/edk2-stable202211-r1/edk2-stable202211-r1-bin.tar.xz";
static X86_64_CODE: &str = "build/ovmf/x86_64/code.fd";
static X86_64_VARS: &str = "build/ovmf/x86_64/vars.fd";
static AARCH64_CODE: &str = "build/ovmf/aarch64/code.fd";
static AARCH64_VARS: &str = "build/ovmf/aarch64/vars.fd";

#[derive(Parser)]
struct Fmt {
    args: Vec<String>,
}

#[derive(Parser)]
#[command(rename_all = "snake_case")]
enum Arguments {
    Clean,
    Update,
    Check,
    Clippy,
    Fmt(Fmt),

    #[command(subcommand)]
    Target(target::Target),

    Build(build::Options),
    Run(run::Options),
}

fn main() -> Result<()> {
    let sh = Shell::new()?;

    let tmp_dir = sh.create_temp_dir()?;

    let mut limine_download = false;
    let mut ovmf_download = false;

    if !sh.path_exists(".debug/") {
        sh.create_dir(".debug/")?;
    }

    // Ensure the binaries have their `.cargo/config.toml`s.
    if !sh.path_exists("src/kernel/.cargo/config.toml") || !sh.path_exists("src/userspace/.cargo/config.toml") {
        target::update_target(&sh, target::Target::x86_64)?;
    }

    // Ensure we use the 'sparse' cargo repository protocol
    sh.set_var("CARGO_REGISTRIES_CRATES_IO_PROTOCOL", "sparse");

    // Validate all of the relevant files
    create_path_if_not_exists(&sh, "build/root/EFI/BOOT/")?;
    create_path_if_not_exists(&sh, "build/root/linuiz/")?;
    // Ensure dev disk image exists.
    if !sh.path_exists("build/disk0.img") {
        cmd!(sh, "qemu-img create -f raw build/disk0.img 256M").run()?;
    }

    // Ensure a valid bootloader configuration exists.
    if !sh.path_exists("build/root/EFI/BOOT/limine.cfg") {
        sh.write_file("build/root/EFI/BOOT/limine.cfg", LIMINE_DEFAULT_CFG)?;
    }

    if !sh.path_exists("build/root/EFI/BOOT/BOOTX64.EFI") {
        download_limine_binary(&sh)?;
        limine_download = true;
    }

    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    if !sh.path_exists(X86_64_CODE)
        || !sh.path_exists(X86_64_VARS)
        || !sh.path_exists(AARCH64_CODE)
        || !sh.path_exists(AARCH64_VARS)
    {
        download_ovmf_binaries(&sh, &tmp_dir)?;
        ovmf_download = true;
    }

    match Arguments::parse() {
        Arguments::Clean => {
            in_workspace_with(&sh, |sh| cmd!(sh, "cargo clean").run().with_context(|| "`cargo clean` failed"))?;
        }

        Arguments::Check => {
            in_workspace_with(&sh, |sh| cmd!(sh, "cargo check --bins").run().with_context(|| "`cargo check` failed"))?;
        }

        Arguments::Update => {
            in_workspace_with(&sh, |sh| cmd!(sh, "cargo update").run().with_context(|| "`cargo update` failed"))?;

            if !limine_download {
                download_limine_binary(&sh)?;
            }

            if !ovmf_download {
                download_ovmf_binaries(&sh, &tmp_dir)?;
            }
        }

        Arguments::Clippy => {
            in_workspace_with(&sh, |sh| cmd!(sh, "cargo clippy").run().with_context(|| "`cargo clippy` failed"))?;
        }

        Arguments::Fmt(fmt) => {
            let args = &fmt.args;
            in_workspace_with(&sh, |sh| cmd!(sh, "cargo fmt {args...}").run().with_context(|| "`cargo fmt` failed"))?;
        }

        Arguments::Target(target) => {
            target::update_target(&sh, target).with_context(|| "failed to update targets")?;
        }

        Arguments::Build(build_options) => {
            build::build(&sh, build_options)?;
        }

        Arguments::Run(run_options) => {
            run::run(&sh, run_options)?;
        }
    }

    Ok(())
}

fn in_workspace_with(shell: &Shell, with_fn: impl Fn(&Shell) -> Result<()>) -> Result<()> {
    for dir in WORKSPACE_DIRS {
        let _dir = shell.push_dir(dir);
        with_fn(shell)?
    }

    Ok(())
}

fn create_path_if_not_exists<P: AsRef<Path>>(sh: &Shell, path: P) -> Result<()> {
    if !sh.path_exists(path.as_ref()) {
        sh.create_dir(path.as_ref())?;
    }

    Ok(())
}

fn download_limine_binary(sh: &Shell) -> Result<()> {
    println!("Downloading limine UEFI boot image.");

    let out_path = "build/root/EFI/BOOT/BOOTX64.EFI";
    let response = reqwest::blocking::get(LIMINE_UEFI_IMAGE_URL)?;
    copy(&mut Cursor::new(response.bytes()?), &mut File::create(out_path)?)?;

    assert!(sh.path_exists(out_path));

    Ok(())
}

fn download_ovmf_binaries(sh: &Shell, tmp_dir: &TempDir) -> Result<()> {
    println!("Downloading UEFI firmware binaries.");

    sh.create_dir("build/ovmf/x86_64/")?;
    sh.create_dir("build/ovmf/aarch64/")?;

    let tar_path = tmp_dir.path().join("ovmf_prebuilts.tar.xz");

    let response = reqwest::blocking::get(UEFI_FIRMWARE_IMAGE_URL)?;
    copy(&mut Cursor::new(response.bytes()?), &mut File::create(tar_path.clone())?)?;

    let mut archive_compressed = BufReader::new(File::open(tar_path)?);
    let mut archive_decompressed = Vec::new();
    lzma_rs::xz_decompress(&mut archive_compressed, &mut archive_decompressed)?;

    let archive_stream = BufReader::new(archive_decompressed.as_slice());
    let mut archive = tar::Archive::new(archive_stream);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.to_string_lossy().into_owned();

        if path.ends_with("x64/code.fd") {
            entry.unpack(X86_64_CODE)?;
        } else if path.ends_with("x64/vars.fd") {
            entry.unpack(X86_64_VARS)?;
        } else if path.ends_with("aarch64/code.fd") {
            entry.unpack(AARCH64_CODE)?;
        } else if path.ends_with("aarch64/vars.fd") {
            entry.unpack(AARCH64_VARS)?;
        }
    }

    assert!(sh.path_exists(X86_64_CODE));
    assert!(sh.path_exists(X86_64_VARS));
    assert!(sh.path_exists(AARCH64_CODE));
    assert!(sh.path_exists(AARCH64_VARS));

    Ok(())
}
