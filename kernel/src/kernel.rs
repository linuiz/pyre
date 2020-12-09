#![no_std]
#![no_main]
#![feature(asm)]

mod drivers;
mod io;

use core::panic::PanicInfo;
use drivers::{serial, vga};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
fn kernel_main() -> i32 {
    vga::safe_lock(|writer| {
        writer.write_string("testssssssssssssss");
    });

    loop {}
}
