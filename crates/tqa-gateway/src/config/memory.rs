//! `[memory]`: long-term memory (ADR-0041) for this installation: each task's sandbox is told to build the workspace's
//! memory rules and index into its runs' instructions. Off, a run sends exactly what it sends without memory. Off by
//! default while the rest of phase 1 is built (notes shown in Files, the secret scan on saves, "Remembered" with Undo);
//! the slice that completes it turns the default on, as the record decides.
use serde::Deserialize;

#[derive(Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct MemoryInput {
    enabled: bool,
}

/// Whether this installation has long-term memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Memory {
    /// The install switch; a person's own switch comes with Settings › Memory.
    pub(crate) enabled: bool,
}

pub(super) fn memory(input: &MemoryInput) -> Memory {
    Memory {
        enabled: input.enabled,
    }
}

#[cfg(test)]
mod tests {
    use super::{MemoryInput, memory};

    #[derive(serde::Deserialize)]
    struct File {
        #[serde(default)]
        memory: MemoryInput,
    }

    fn enabled(text: &str) -> Result<bool, toml::de::Error> {
        toml::from_str::<File>(text).map(|file| memory(&file.memory).enabled)
    }

    // TEST: GW-U-231
    #[test]
    fn gw_u_231_memory_is_off_until_switched_on() -> Result<(), toml::de::Error> {
        assert!(!enabled("")?, "no section: off while phase 1 is built");
        assert!(!enabled("[memory]\n")?, "an empty section: off");
        assert!(!enabled("[memory]\nenabled = false\n")?);
        assert!(enabled("[memory]\nenabled = true\n")?);
        Ok(())
    }

    // TEST: GW-U-231
    #[test]
    fn gw_u_231_an_unknown_key_is_refused() {
        assert!(enabled("[memory]\nenable = false\n").is_err());
    }
}
