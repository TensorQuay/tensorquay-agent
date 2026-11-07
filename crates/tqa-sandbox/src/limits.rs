use crate::SandboxError;

/// Positive bytes representable by the Docker Engine's signed limit fields.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteSize {
    bytes: u64,
    signed: i64,
}

impl ByteSize {
    /// Constructs a bounded positive byte count.
    ///
    /// # Errors
    /// Returns `InvalidSpec` for zero or counts above the Engine's signed range.
    pub fn new(bytes: u64) -> Result<Self, SandboxError> {
        let signed = i64::try_from(bytes).map_err(|_| {
            SandboxError::invalid("memory", "byte count exceeds the supported range")
        })?;
        if signed == 0 {
            return Err(SandboxError::invalid(
                "memory",
                "byte count must be positive",
            ));
        }
        Ok(Self { bytes, signed })
    }

    /// Returns the byte count without conversion or loss.
    #[must_use]
    pub const fn bytes(self) -> u64 {
        self.bytes
    }

    pub(crate) const fn signed(self) -> i64 {
        self.signed
    }
}

/// CPU bandwidth in millicores, from one millicore through one thousand cores.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CpuLimit(u32);

impl CpuLimit {
    /// Validates a CPU bandwidth request.
    ///
    /// # Errors
    /// Returns `InvalidSpec` outside 1 through 1,000,000 millicores.
    pub fn new(millicores: u32) -> Result<Self, SandboxError> {
        if !(1..=1_000_000).contains(&millicores) {
            return Err(SandboxError::invalid(
                "cpu",
                "expected 1 through 1000000 millicores",
            ));
        }
        Ok(Self(millicores))
    }

    /// Returns the validated bandwidth in millicores.
    #[must_use]
    pub const fn millicores(self) -> u32 {
        self.0
    }
}
