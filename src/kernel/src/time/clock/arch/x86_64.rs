use crate::{acpi::Register, time::clock::Clock};
use core::{
    sync::atomic::{AtomicU32, AtomicU64, Ordering},
    time::Duration,
};

// TODO use `error` crate rather than `errorgen`

pub struct Acpi<'a> {
    register: Register<'a, u32>,
    frequency: u32,
    max_timestamp: u32,
    last_timestamp: AtomicU32,
    rollovers: AtomicU64,
}

// Safety: Utilizes global memory and atomics.
unsafe impl Send for Acpi<'_> {}
// Safety: Utilizes global memory and atomics.
unsafe impl Sync for Acpi<'_> {}

impl Acpi<'_> {
    pub fn load() -> Option<Self> {
        let fadt = crate::acpi::get_fadt();
        let pm_timer = acpi::platform::PmTimer::new(fadt).ok()?;

        if let Some(pm_timer) = pm_timer.as_ref()
        && let Some(register) = crate::acpi::Register::new(&pm_timer.base)
        {
            Some(Self {
                register,
                frequency: 3579545,
                max_timestamp: if pm_timer.supports_32bit { u32::MAX } else { 0x00FFFFFF },
                last_timestamp: AtomicU32::new(0),
                rollovers: AtomicU64::new(0),
            })
        } else {
            None
        }
    }
}

impl Clock for Acpi<'_> {
    #[inline]
    fn frequency(&self) -> u64 {
        self.frequency.into()
    }

    #[inline]
    fn get_timestamp(&self) -> u64 {
        let cur_timestamp = self.register.read();
        let last_timestamp = self.last_timestamp.swap(cur_timestamp, Ordering::AcqRel);

        if cur_timestamp < last_timestamp {
            self.rollovers.fetch_add(1, Ordering::Release);
        }

        let max_timestamp = u64::from(self.max_timestamp);
        let rollovers = self.rollovers.load(Ordering::Acquire);
        (rollovers * max_timestamp) + u64::from(cur_timestamp)
    }

    fn spin_wait(&self, duration: Duration) {
        let ticks_per_ms = self.frequency() / 1000;
        let duration_ticks = u64::try_from(duration.as_millis()).unwrap() * ticks_per_ms;

        let end_timestamp = self.get_timestamp() + duration_ticks;
        while self.get_timestamp() < end_timestamp {
            core::hint::spin_loop();
        }
    }
}
