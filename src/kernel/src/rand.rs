#![allow(clippy::no_mangle_with_rust_abi)]

getrandom::register_custom_getrandom!(prng_custom_getrandom);

#[allow(clippy::unnecessary_wraps)]
fn prng_custom_getrandom(buf: &mut [u8]) -> Result<(), getrandom::Error> {
    for (index, chunk) in buf.chunks_mut(core::mem::size_of::<u64>()).enumerate() {
        let rng_bytes = prng::next_u64().to_ne_bytes();
        chunk.copy_from_slice(&rng_bytes[..chunk.len()]);
    }

    Ok(())
}

pub mod prng {
    use rand_core::RngCore;
    use rand_pcg::Pcg64Mcg;
    use spin::{Lazy, Mutex};

    static PCG: Lazy<Mutex<Pcg64Mcg>> = Lazy::new(|| {
        Mutex::new(Pcg64Mcg::new({
            #[cfg(target_arch = "x86_64")]
            {
                // Safety: ???
                unsafe {
                    let state_low = u128::from(core::arch::x86_64::_rdtsc());

                    for _ in 0..(state_low & 0xFF) {
                        core::hint::spin_loop();
                    }

                    let state_high = u128::from(core::arch::x86_64::_rdtsc());
                    state_low | (state_high << 64)
                }
            }
        }))
    });

    pub fn next_u32() -> u32 {
        PCG.lock().next_u32()
    }

    pub fn next_u64() -> u64 {
        PCG.lock().next_u64()
    }
}
