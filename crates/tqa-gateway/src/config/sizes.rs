//! An environment's size (the environment sizes design): CPUs in halves, memory, processes and the disk of a workspace
//! created for it, each bounded here and by the installation's `[sandbox.ceiling]`, which no environment may exceed.
use serde::{Deserialize, Deserializer};

const MIB: u64 = 1024 * 1024;

/// Half a CPU, the step of every CPU size.
const HALF_CPU_MILLICORES: u32 = 500;
/// The most CPUs an environment may have (64, in halves).
const MAX_HALF_CPUS: u32 = 128;
pub(crate) const MEMORY_MIB: std::ops::RangeInclusive<u64> = 256..=1_048_576;
pub(crate) const PIDS: std::ops::RangeInclusive<u32> = 64..=65_536;
pub(crate) const WORKSPACE_MIB: std::ops::RangeInclusive<u64> = 16..=1_048_576;

/// `cpus` as millicores when it is a positive multiple of 0.5 up to 64; `None` otherwise.
pub(crate) fn cpu_millicores(cpus: f64) -> Option<u32> {
    let halves = cpus * 2.0;
    (1..=MAX_HALF_CPUS)
        .find(|candidate| (f64::from(*candidate) - halves).abs() < 1e-9)
        .map(|found| found * HALF_CPU_MILLICORES)
}

/// Millicores as the `cpus` people read (0.5, 1, 1.5, …).
pub(crate) fn cpus(millicores: u32) -> f64 {
    f64::from(millicores) / 1000.0
}

/// A size in whole MiB, as bytes, when within `bounds`.
pub(crate) fn mib_bytes(mib: u64, bounds: &std::ops::RangeInclusive<u64>) -> Option<u64> {
    bounds.contains(&mib).then(|| mib * MIB)
}

/// The installation's ceiling (file-only): no environment is sized above it, and the defaults are within it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Ceiling {
    cpu_millicores: u32,
    memory_bytes: u64,
    pids: u32,
    workspace_bytes: u64,
}

impl Default for Ceiling {
    fn default() -> Self {
        Self {
            cpu_millicores: 8_000,
            memory_bytes: 16_384 * MIB,
            pids: 4_096,
            workspace_bytes: 102_400 * MIB,
        }
    }
}

impl Ceiling {
    pub(crate) const fn cpu_millicores(&self) -> u32 {
        self.cpu_millicores
    }
    pub(crate) const fn memory_bytes(&self) -> u64 {
        self.memory_bytes
    }
    pub(crate) const fn pids(&self) -> u32 {
        self.pids
    }
    pub(crate) const fn workspace_bytes(&self) -> u64 {
        self.workspace_bytes
    }

    /// The first field an environment's own size puts above this ceiling.
    pub(crate) fn exceeded_by(&self, size: &Size) -> Option<&'static str> {
        self.exceeded(size).into_iter().next()
    }

    /// Every field an environment's own size puts above this ceiling (after the file lowered it), in field order.
    pub(crate) fn exceeded(&self, size: &Size) -> Vec<&'static str> {
        let above = |own: Option<u64>, most: u64| own.is_some_and(|own| own > most);
        [
            (
                "cpus",
                above(
                    size.cpu_millicores.map(u64::from),
                    u64::from(self.cpu_millicores),
                ),
            ),
            ("memory_mib", above(size.memory_bytes, self.memory_bytes)),
            (
                "pids",
                above(size.pids.map(u64::from), u64::from(self.pids)),
            ),
            (
                "workspace_mib",
                above(size.workspace_bytes, self.workspace_bytes),
            ),
        ]
        .into_iter()
        .filter_map(|(field, over)| over.then_some(field))
        .collect()
    }
}

/// An environment's own size; `None` fields use the installation's defaults.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Size {
    pub cpu_millicores: Option<u32>,
    pub memory_bytes: Option<u64>,
    pub pids: Option<u32>,
    pub workspace_bytes: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CeilingInput {
    #[serde(default)]
    cpus: Option<f64>,
    #[serde(default)]
    memory_mib: Option<u64>,
    #[serde(default)]
    pids: Option<u32>,
    #[serde(default)]
    workspace_mib: Option<u64>,
}

impl<'de> Deserialize<'de> for Ceiling {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let input = CeilingInput::deserialize(deserializer)?;
        let fallback = Self::default();
        let bad = |field: &str| {
            serde::de::Error::custom(format!("[sandbox.ceiling] {field} is out of range"))
        };
        Ok(Self {
            cpu_millicores: input
                .cpus
                .map_or(Some(fallback.cpu_millicores), cpu_millicores)
                .ok_or_else(|| bad("cpus"))?,
            memory_bytes: input
                .memory_mib
                .map_or(Some(fallback.memory_bytes), |mib| {
                    mib_bytes(mib, &MEMORY_MIB)
                })
                .ok_or_else(|| bad("memory_mib"))?,
            pids: input
                .pids
                .map_or(Some(fallback.pids), |pids| {
                    PIDS.contains(&pids).then_some(pids)
                })
                .ok_or_else(|| bad("pids"))?,
            workspace_bytes: input
                .workspace_mib
                .map_or(Some(fallback.workspace_bytes), |mib| {
                    mib_bytes(mib, &WORKSPACE_MIB)
                })
                .ok_or_else(|| bad("workspace_mib"))?,
        })
    }
}

/// The installation's defaults and ceiling as the admin API shows them (the environment form and the Limits page).
pub(crate) fn limits(config: &crate::GatewayConfig) -> serde_json::Value {
    let sandbox = &config.sandbox;
    let ceiling = &sandbox.ceiling;
    serde_json::json!({
        "defaults": {
            "cpus": cpus(sandbox.cpu_millicores),
            "memory_mib": sandbox.memory_bytes / MIB,
            "pids": sandbox.pids,
            "workspace_mib": config.workspaces.quota_bytes / MIB,
        },
        "ceiling": {
            "cpus": cpus(ceiling.cpu_millicores),
            "memory_mib": ceiling.memory_bytes / MIB,
            "pids": ceiling.pids,
            "workspace_mib": ceiling.workspace_bytes / MIB,
        },
    })
}
