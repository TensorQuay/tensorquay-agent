//! Internet-access presets (F20): built-in, named rule sets an administrator applies in one step. They are code, reviewed
//! like code, never configuration. Applying one adds its missing rules tagged with it and keeps a host that already
//! has a rule; removing one deletes the rules still tagged. An administrator's edit makes a rule theirs
//! (`preset_edited`), so removal keeps it. `open_web` is the only way the any-host rule '*' is stored.
use super::{Change, audit, egress::MAX_RULES};
use crate::{GatewayError, egress::ANY_HOST};
use serde::Serialize;
use sqlx::PgConnection;

/// One preset: its id (the web names and describes it) and its rules.
pub(crate) struct Preset {
    pub id: &'static str,
    pub rules: &'static [(&'static str, &'static [u16])],
}

const HTTPS: &[u16] = &[443];

/// Every preset, in the order the page lists them.
pub(crate) const PRESETS: [Preset; 3] = [
    // Installing packages only: Python, JavaScript, Rust and Go.
    Preset {
        id: "package_registries",
        rules: &[
            ("pypi.org", HTTPS),
            ("files.pythonhosted.org", HTTPS),
            ("registry.npmjs.org", HTTPS),
            ("crates.io", HTTPS),
            ("static.crates.io", HTTPS),
            ("index.crates.io", HTTPS),
            ("proxy.golang.org", HTTPS),
            ("sum.golang.org", HTTPS),
        ],
    },
    // Any website on the web's two ports. Never "read-only": over TLS the proxy sees only the host and port, so the
    // agent can send data to any site. A name that resolves to a private or reserved address stays refused.
    Preset {
        id: "open_web",
        rules: &[(ANY_HOST, &[80, 443])],
    },
    Preset {
        id: "code_hosting",
        rules: &[
            ("github.com", HTTPS),
            ("codeload.github.com", HTTPS),
            ("raw.githubusercontent.com", HTTPS),
            ("objects.githubusercontent.com", HTTPS),
            ("gitlab.com", HTTPS),
        ],
    },
];

/// The preset of this id.
pub(crate) fn find(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|preset| preset.id == id)
}

/// How much of a preset is in the allow-list as it put it there.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Applied {
    None,
    Partial,
    All,
}

/// A rule as a preset answer names it.
#[derive(Clone, Debug, Serialize)]
pub(crate) struct Rule {
    pub host: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ports: Option<Vec<u16>>,
}

fn rule(host: &str, ports: Option<&[u16]>) -> Rule {
    Rule {
        host: host.to_owned(),
        ports: ports.map(<[u16]>::to_vec),
    }
}

/// Each preset with how much of it is applied: its rules still tagged with it and unedited, of all of them.
///
/// # Errors
/// Storage failure.
pub(crate) async fn applied(
    connection: &mut PgConnection,
) -> Result<Vec<(&'static Preset, Applied)>, GatewayError> {
    let tagged = sqlx::query!(
        "SELECT host,preset AS \"preset!\" FROM egress_rules WHERE preset IS NOT NULL AND NOT preset_edited"
    )
    .fetch_all(connection)
    .await?;
    Ok(PRESETS
        .iter()
        .map(|preset| {
            let present = preset
                .rules
                .iter()
                .filter(|(host, _)| {
                    tagged
                        .iter()
                        .any(|row| row.preset == preset.id && row.host == *host)
                })
                .count();
            let applied = match present {
                0 => Applied::None,
                count if count == preset.rules.len() => Applied::All,
                _ => Applied::Partial,
            };
            (preset, applied)
        })
        .collect())
}

/// Adds the preset's rules its allow-list lacks, tagged with it and audited as the actor's creates, in the caller's
/// settings transaction. A host that already has a rule, the administrator's or another preset's, is kept as it is.
/// Returns the rules added and the hosts kept.
///
/// # Errors
/// `409 egress_rule_limit` when the list would pass its bound, storage failure.
pub(crate) async fn apply(
    connection: &mut PgConnection,
    preset: &Preset,
    actor: uuid::Uuid,
) -> Result<(Vec<Rule>, Vec<Rule>), GatewayError> {
    let (mut added, mut kept) = (Vec::new(), Vec::new());
    for (host, ports) in preset.rules {
        let exists = sqlx::query_scalar!(
            "SELECT host FROM egress_rules WHERE host=$1 FOR UPDATE",
            host
        )
        .fetch_optional(&mut *connection)
        .await?
        .is_some();
        if exists {
            kept.push(rule(host, None));
            continue;
        }
        let count = sqlx::query_scalar!("SELECT count(*) AS \"count!\" FROM egress_rules")
            .fetch_one(&mut *connection)
            .await?;
        if count >= MAX_RULES {
            return Err(GatewayError::Conflict("egress_rule_limit"));
        }
        let stored: Vec<i32> = ports.iter().copied().map(i32::from).collect();
        sqlx::query!(
            "INSERT INTO egress_rules (host,ports,version,last_set_by,preset) VALUES ($1,$2,1,'admin',$3)",
            host,
            &stored,
            preset.id
        )
        .execute(&mut *connection)
        .await?;
        let change = Change {
            area: "egress",
            actor: Some(actor),
            item: host,
            action: "create",
            before: None,
            after: Some(1),
            value_changed: true,
        };
        audit(connection, &change).await?;
        added.push(rule(host, Some(ports)));
    }
    Ok((added, kept))
}

/// Removes the rules still tagged with the preset and unedited, audited as the actor's deletes, in the caller's settings
/// transaction. Returns the hosts removed and those kept (tagged, but an administrator edited them since).
///
/// # Errors
/// Storage failure.
pub(crate) async fn remove(
    connection: &mut PgConnection,
    preset: &Preset,
    actor: uuid::Uuid,
) -> Result<(Vec<Rule>, Vec<Rule>), GatewayError> {
    let rows = sqlx::query!(
        "SELECT host,version,preset_edited FROM egress_rules WHERE preset=$1 ORDER BY host FOR UPDATE",
        preset.id
    )
    .fetch_all(&mut *connection)
    .await?;
    let (mut removed, mut kept) = (Vec::new(), Vec::new());
    for row in rows {
        if row.preset_edited {
            kept.push(rule(&row.host, None));
            continue;
        }
        sqlx::query!("DELETE FROM egress_rules WHERE host=$1", row.host)
            .execute(&mut *connection)
            .await?;
        let change = Change {
            area: "egress",
            actor: Some(actor),
            item: &row.host,
            action: "delete",
            before: Some(row.version),
            after: None,
            value_changed: true,
        };
        audit(connection, &change).await?;
        removed.push(rule(&row.host, None));
    }
    Ok((removed, kept))
}
