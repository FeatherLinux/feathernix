//! Shared cryptographic utilities and RNG compatibility for FeatherNix.

use rand::rngs::SysRng;
use rand::{TryCryptoRng, TryRng};
use rand_core_06::{CryptoRng, Error, RngCore};

/// OS randomness backed by Rand 0.10, compatible with Rand Core 0.6 consumers.
///
/// Implements the older RNG traits required by `pgp` as well as the modern
/// fallible traits. Fallible calls return entropy errors; the legacy infallible
/// methods panic on failure, just like Rand 0.8's `OsRng`.
#[derive(Clone, Copy, Debug, Default)]
pub struct CompatOsRng;

impl TryRng for CompatOsRng {
    type Error = <SysRng as TryRng>::Error;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        SysRng.try_next_u32()
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        SysRng.try_next_u64()
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Self::Error> {
        SysRng.try_fill_bytes(dest)
    }
}

impl TryCryptoRng for CompatOsRng {}

impl RngCore for CompatOsRng {
    fn next_u32(&mut self) -> u32 {
        self.try_next_u32().expect("OS randomness unavailable")
    }

    fn next_u64(&mut self) -> u64 {
        self.try_next_u64().expect("OS randomness unavailable")
    }

    fn fill_bytes(&mut self, dest: &mut [u8]) {
        TryRng::try_fill_bytes(self, dest).expect("OS randomness unavailable");
    }

    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), Error> {
        TryRng::try_fill_bytes(self, dest).map_err(Error::new)
    }
}

impl CryptoRng for CompatOsRng {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_crypto_rng_supports_os_randomness() {
        fn require_crypto_rng<T: RngCore + CryptoRng>(_: &T) {}
        let mut rng = CompatOsRng;
        require_crypto_rng(&rng);
        let mut bytes = [0u8; 64];
        RngCore::try_fill_bytes(&mut rng, &mut bytes).unwrap();
        assert_ne!(bytes, [0; 64]);
        RngCore::fill_bytes(&mut rng, &mut []);
        let _ = rng.next_u32();
        let _ = rng.next_u64();
    }

    #[test]
    fn modern_crypto_rng_supports_os_randomness() {
        fn require_crypto_rng<T: TryCryptoRng>(_: &T) {}
        let mut rng = CompatOsRng;
        require_crypto_rng(&rng);
        let mut bytes = [0u8; 64];
        TryRng::try_fill_bytes(&mut rng, &mut bytes).unwrap();
        assert_ne!(bytes, [0; 64]);
        TryRng::try_fill_bytes(&mut rng, &mut []).unwrap();
        rng.try_next_u32().unwrap();
        rng.try_next_u64().unwrap();
    }
}
