use core::ops::{Div, Rem};

use crate::interrupts::InterruptCell;
use spin::Mutex;
use uart::{Data, Uart, UartWriter};

pub struct Serial(InterruptCell<Mutex<UartWriter>>);

// Safety: Interior address is not thread-specific.
unsafe impl Send for Serial {}
// Safety: This isn't actually safe. It relies entirely on only
//         one `Serial` being created and used at a time.
//         So basically, TODO.
unsafe impl Sync for Serial {}

impl log::Log for Serial {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let cpu = crate::cpu::read_id();

            let now = crate::time::clock::Instant::now();
            let secs = now.as_secs();
            let millis = now.as_millis() - (secs * 1000);

            self.0.with(|uart| {
                use core::fmt::Write;

                let mut uart = uart.lock();

                uart.write_fmt(format_args!(
                    "[{cpu:>3}][{secs}.{millis:0<3}][{level:>5}] {args}\n",
                    level = record.level(),
                    args = record.args(),
                ))
                .unwrap();
            });
        }
    }

    fn flush(&self) {}
}

errorgen! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Error {
        SetLogger => None,
        NoLogger => None
    }
}

pub fn init() -> Result<()> {
    #[cfg(debug_assertions)]
    {
        log::set_max_level(log::LevelFilter::Trace);
    }
    #[cfg(not(debug_assertions))]
    {
        log::set_max_level(log::LevelFilter::Trace);
    }

    static SERIAL_UART: spin::Lazy<Option<Serial>> = spin::Lazy::new(|| {
        crate::interrupts::without(|| {
            UartWriter::new(
                #[cfg(target_arch = "x86_64")]
                // Safety: Constructor is called only once, with a hopefully-valid address.
                unsafe {
                    Uart::<Data>::new(uart::COM1)
                },
            )
            .map(Mutex::new)
            .map(InterruptCell::new)
            .map(Serial)
        })
    });

    let uart = SERIAL_UART.as_ref().ok_or(Error::NoLogger)?;
    log::set_logger(uart).map_err(|_| Error::SetLogger)?;

    Ok(())
}
