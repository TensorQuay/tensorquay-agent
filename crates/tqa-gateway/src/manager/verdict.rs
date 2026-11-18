//! The leak verdict (the Sandboxes page design note, §3.6): what the last two sweeps found that no record accounts for,
//! and what survived them. Green when nothing unowned survived a sweep, amber when something did, never red: the
//! reaper's rules protect the slots, the verdict protects the host's disk and address pool. Pure; the sweeps record
//! their passes and the readiness detail, the metrics and the listing read the verdict.
use serde::Serialize;
use time::OffsetDateTime;

/// What kind of engine object a sweep found.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Kind {
    Container,
    Network,
    /// A workspace volume in volume mode: the volume sweep that finds them isn't built yet (SBX-U-019), so only the
    /// verdict's tests make one today.
    #[cfg_attr(not(test), allow(dead_code))]
    Volume,
}

impl Kind {
    /// The command that removes the object by hand, built from the engine's name only.
    pub(crate) fn remove(self, name: &str) -> String {
        match self {
            // Forced: plain `docker rm` refuses a paused or a running container, which is what a survivor often is.
            Self::Container => format!("docker rm -f {name}"),
            Self::Network => format!("docker network rm {name}"),
            Self::Volume => format!("docker volume rm {name}"),
        }
    }
}

/// One unowned object a pass met: removed (`Ok`), or not, with the engine's error code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Finding {
    pub kind: Kind,
    /// A container's short id, a network's or a volume's name: the engine's, never a task's.
    pub name: String,
    pub age_seconds: u64,
    pub outcome: Result<(), &'static str>,
}

/// One sweep pass: when, what was running, and what it found.
#[derive(Clone, Debug)]
pub(crate) struct Pass {
    pub at: OffsetDateTime,
    pub running: u64,
    pub oldest_seconds: u64,
    /// Whether volumes were looked at (the volume workspace mode); in bind mode there are none to count.
    pub volumes: bool,
    pub findings: Vec<Finding>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Tone {
    #[default]
    Green,
    Amber,
}

/// Counts by kind; `volumes` only when volumes were looked at.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
pub(crate) struct Counts {
    pub containers: u64,
    pub networks: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volumes: Option<u64>,
}

impl Counts {
    fn add(&mut self, kind: Kind) {
        match kind {
            Kind::Container => self.containers += 1,
            Kind::Network => self.networks += 1,
            Kind::Volume => self.volumes = Some(self.volumes.unwrap_or(0) + 1),
        }
    }
}

/// Something that survived a sweep: named for the page, with the command that removes it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct Survivor {
    pub kind: Kind,
    pub name: String,
    pub age_seconds: u64,
    pub reason: &'static str,
    pub remove: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Verdict {
    pub tone: Tone,
    pub running: u64,
    pub oldest_seconds: u64,
    pub unowned: Counts,
    pub left_behind: Vec<Survivor>,
    pub swept_at: Option<OffsetDateTime>,
}

impl Verdict {
    pub(crate) fn left_behind_counts(&self) -> Counts {
        let mut counts = Counts {
            volumes: self.unowned.volumes.map(|_| 0),
            ..Counts::default()
        };
        for survivor in &self.left_behind {
            counts.add(survivor.kind);
        }
        counts
    }

    /// The oldest survivor's age.
    pub(crate) fn oldest_left_behind_seconds(&self) -> Option<u64> {
        self.left_behind.iter().map(|s| s.age_seconds).max()
    }

    /// The oldest survivor's reason.
    pub(crate) fn reason(&self) -> Option<&'static str> {
        self.left_behind
            .iter()
            .max_by_key(|s| s.age_seconds)
            .map(|s| s.reason)
    }

    /// The live counts, read at the time of the answer rather than at the last sweep.
    pub(crate) fn with_running(mut self, running: u64, oldest_seconds: u64) -> Self {
        self.running = running;
        self.oldest_seconds = oldest_seconds;
        self
    }

    /// The `sandboxes` object of `/readyz?detail=1`: the counts only, never a name.
    pub(crate) fn readiness(&self) -> serde_json::Value {
        let left = self.left_behind_counts();
        let mut left_behind = serde_json::json!({
            "containers": left.containers,
            "networks": left.networks,
            "oldest_seconds": self.oldest_left_behind_seconds(),
            "reason": self.reason(),
        });
        if let Some(volumes) = left.volumes {
            left_behind["volumes"] = serde_json::json!(volumes);
        }
        serde_json::json!({
            "running": self.running,
            "oldest_seconds": self.oldest_seconds,
            "unowned": self.unowned,
            "left_behind": left_behind,
            "swept_at": self.swept_at.and_then(|at| at.format(&time::format_description::well_known::Rfc3339).ok()),
        })
    }
}

/// The verdict from the last pass and the one before it: a finding is a survivor when the last pass could not remove
/// it and the previous pass had already met it (it failed twice, or it was found again after a pass tried it).
pub(crate) fn verdict(previous: Option<&Pass>, last: &Pass) -> Verdict {
    let mut unowned = Counts {
        volumes: last.volumes.then_some(0),
        ..Counts::default()
    };
    let mut left_behind = Vec::new();
    for finding in &last.findings {
        unowned.add(finding.kind);
        let Err(reason) = finding.outcome else {
            continue;
        };
        let met_before = previous.is_some_and(|pass| {
            pass.findings
                .iter()
                .any(|earlier| earlier.kind == finding.kind && earlier.name == finding.name)
        });
        if met_before {
            left_behind.push(Survivor {
                kind: finding.kind,
                name: finding.name.clone(),
                age_seconds: finding.age_seconds,
                reason,
                remove: finding.kind.remove(&finding.name),
            });
        }
    }
    Verdict {
        tone: if left_behind.is_empty() {
            Tone::Green
        } else {
            Tone::Amber
        },
        running: last.running,
        oldest_seconds: last.oldest_seconds,
        unowned,
        left_behind,
        swept_at: Some(last.at),
    }
}

/// The last two passes, kept in memory by the manager.
#[derive(Debug, Default)]
pub(crate) struct History {
    previous: Option<Pass>,
    last: Option<Pass>,
}

impl History {
    pub(crate) fn record(&mut self, pass: Pass) {
        self.previous = self.last.take();
        self.last = Some(pass);
    }

    /// The verdict as of the last pass; green with nothing swept before the first.
    pub(crate) fn verdict(&self) -> Verdict {
        self.last.as_ref().map_or_else(Verdict::default, |last| {
            verdict(self.previous.as_ref(), last)
        })
    }
}
