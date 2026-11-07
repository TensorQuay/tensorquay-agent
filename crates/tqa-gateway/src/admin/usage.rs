//! `GET /api/v1/admin/usage` (090): relayed-request usage by day, user or route over an inclusive UTC date range.
use super::require_admin;
use crate::{GatewayError, InvalidReason, app::State, auth::Authz};
use axum::{
    Json,
    extract::{RawQuery, State as AppState},
};
use serde::Serialize;
use std::sync::Arc;
use time::{Date, Duration, OffsetDateTime, macros::format_description};

/// The longest range, in days between `from` and `to`, so a day report has at most 93 rows.
const MAX_SPAN_DAYS: i64 = 92;
/// Rows in a user or route report.
const MAX_ROWS: usize = 200;
/// Days in the default range, ending today.
const DEFAULT_DAYS: i64 = 30;

#[derive(Clone, Copy)]
enum Group {
    Day,
    User,
    Route,
}

impl Group {
    const fn name(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::User => "user",
            Self::Route => "route",
        }
    }
}

/// One report row; token counts are summed over rows that reported them, and the optional ones stay null when no row
/// in the group reported them.
#[derive(Serialize)]
pub(crate) struct Row {
    key: String,
    label: Option<String>,
    requests: i64,
    /// Rows whose counts the model server reported.
    reported: i64,
    /// Rows whose counts are the relay's estimate: the server reported none (a step cancelled mid-reply).
    estimated: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cached_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
}

#[derive(Serialize)]
pub(crate) struct Totals {
    requests: i64,
    /// Rows whose counts the model server reported.
    reported: i64,
    /// Rows whose counts are the relay's estimate: the server reported none (a step cancelled mid-reply).
    estimated: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cached_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
}

#[derive(Serialize)]
pub(crate) struct Report {
    from: String,
    to: String,
    group: &'static str,
    rows: Vec<Row>,
    totals: Totals,
    truncated: bool,
    /// With `compare=previous`: the equal-length period just before, and its totals (GW-I-118).
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous: Option<Totals>,
}

struct Range {
    from: Date,
    to: Date,
    group: Group,
    /// `compare=previous`: also total the equal-length period just before.
    compare: bool,
}

impl Range {
    /// The equal-length period that ends the day before `from`.
    fn previous(&self) -> (Date, Date) {
        let days = (self.to - self.from).whole_days();
        let to = self.from - Duration::DAY;
        (to - Duration::days(days), to)
    }
}

/// The instants bounding an inclusive date range, in UTC.
fn instants((from, to): (Date, Date)) -> (OffsetDateTime, OffsetDateTime) {
    (
        from.midnight().assume_utc(),
        (to + Duration::DAY).midnight().assume_utc(),
    )
}

async fn period_totals(
    pool: &sqlx::PgPool,
    (start, end): (OffsetDateTime, OffsetDateTime),
) -> Result<Totals, GatewayError> {
    Ok(sqlx::query_as!(
        Totals,
        "SELECT count(*) AS \"requests!\", \
         count(*) FILTER (WHERE (prompt_tokens IS NOT NULL OR completion_tokens IS NOT NULL) AND NOT estimated) AS \"reported!\", \
         count(*) FILTER (WHERE estimated) AS \"estimated!\", \
         coalesce(sum(prompt_tokens),0)::bigint AS \"prompt_tokens!\", \
         coalesce(sum(completion_tokens),0)::bigint AS \"completion_tokens!\", \
         sum(cached_tokens)::bigint AS cached_tokens, sum(reasoning_tokens)::bigint AS reasoning_tokens \
         FROM usage WHERE created_at >= $1 AND created_at < $2",
        start,
        end
    )
    .fetch_one(pool)
    .await?)
}

fn invalid(field: &'static str, reason: InvalidReason) -> GatewayError {
    GatewayError::Invalid { field, reason }
}

fn date(field: &'static str, value: &str) -> Result<Date, GatewayError> {
    Date::parse(value, format_description!("[year]-[month]-[day]"))
        .map_err(|_| invalid(field, InvalidReason::DateInvalid))
}

/// Parses `from`, `to` and `group`; any other or repeated parameter is refused.
fn range(query: Option<&str>, today: Date) -> Result<Range, GatewayError> {
    let (mut from, mut to, mut group, mut compare) = (None, None, None, None);
    for (name, value) in url::form_urlencoded::parse(query.unwrap_or_default().as_bytes()) {
        let slot = match name.as_ref() {
            "from" => &mut from,
            "to" => &mut to,
            "group" => &mut group,
            "compare" => &mut compare,
            _ => return Err(invalid("query", InvalidReason::ParameterInvalid)),
        };
        if slot.replace(value).is_some() {
            return Err(invalid("query", InvalidReason::ParameterInvalid));
        }
    }
    let group = match group.as_deref() {
        None | Some("day") => Group::Day,
        Some("user") => Group::User,
        Some("route") => Group::Route,
        Some(_) => return Err(invalid("group", InvalidReason::GroupInvalid)),
    };
    let to = to.map_or(Ok(today), |value| date("to", &value))?;
    let from = match from {
        Some(value) => date("from", &value)?,
        None => to - Duration::days(DEFAULT_DAYS - 1),
    };
    if to > today + Duration::DAY {
        return Err(invalid("to", InvalidReason::DateFuture));
    }
    if from > to {
        return Err(invalid("from", InvalidReason::RangeReversed));
    }
    if (to - from).whole_days() > MAX_SPAN_DAYS {
        return Err(invalid("to", InvalidReason::RangeTooLong));
    }
    let compare = match compare.as_deref() {
        None => false,
        Some("previous") => true,
        Some(_) => return Err(invalid("compare", InvalidReason::ParameterInvalid)),
    };
    Ok(Range {
        from,
        to,
        group,
        compare,
    })
}

/// `GET /api/v1/admin/usage?from&to&group` (administrator, session or token).
///
/// # Errors
/// `400 invalid_request` with `details`, not an administrator, or a store failure.
pub(crate) async fn usage(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    RawQuery(query): RawQuery,
) -> Result<Json<Report>, GatewayError> {
    require_admin(&user)?;
    let range = range(query.as_deref(), OffsetDateTime::now_utc().date())?;
    let (start, end) = instants((range.from, range.to));
    let previous = range.compare.then(|| range.previous());
    let (mut rows, totals, before) =
        crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
            let rows = rows(
                &state.pool,
                range.group,
                (range.from, range.to),
                (start, end),
            )
            .await?;
            let totals = period_totals(&state.pool, (start, end)).await?;
            let before = match previous {
                Some(period) => Some(period_totals(&state.pool, instants(period)).await?),
                None => None,
            };
            Ok::<_, GatewayError>((rows, totals, before))
        })
        .await?;
    let truncated = rows.len() > MAX_ROWS;
    rows.truncate(MAX_ROWS);
    Ok(Json(Report {
        from: range.from.to_string(),
        to: range.to.to_string(),
        group: range.group.name(),
        rows,
        totals,
        truncated,
        previous_from: previous.map(|(from, _)| from.to_string()),
        previous_to: previous.map(|(_, to)| to.to_string()),
        previous: before,
    }))
}

/// The grouped rows; user and route reports fetch one row past the cap so truncation is visible.
async fn rows(
    pool: &sqlx::PgPool,
    group: Group,
    (from, to): (Date, Date),
    (start, end): (OffsetDateTime, OffsetDateTime),
) -> Result<Vec<Row>, GatewayError> {
    let limit = i64::try_from(MAX_ROWS + 1).unwrap_or(i64::MAX);
    let rows = match group {
        Group::Day => sqlx::query_as!(
            Row,
            "SELECT to_char(d.day, 'YYYY-MM-DD') AS \"key!\", NULL::text AS label, count(u.id) AS \"requests!\", \
             count(u.id) FILTER (WHERE (u.prompt_tokens IS NOT NULL OR u.completion_tokens IS NOT NULL) AND NOT coalesce(u.estimated, false)) AS \"reported!\", \
             count(u.id) FILTER (WHERE u.estimated) AS \"estimated!\", \
             coalesce(sum(u.prompt_tokens),0)::bigint AS \"prompt_tokens!\", \
             coalesce(sum(u.completion_tokens),0)::bigint AS \"completion_tokens!\", \
             sum(u.cached_tokens)::bigint AS cached_tokens, sum(u.reasoning_tokens)::bigint AS reasoning_tokens \
             FROM generate_series($1::date, $2::date, interval '1 day') AS d(day) \
             LEFT JOIN usage u ON u.created_at >= d.day AT TIME ZONE 'UTC' \
             AND u.created_at < (d.day + interval '1 day') AT TIME ZONE 'UTC' \
             GROUP BY d.day ORDER BY d.day",
            from,
            to
        )
        .fetch_all(pool)
        .await?,
        Group::User => sqlx::query_as!(
            Row,
            "SELECT COALESCE(u.user_id, c.owner_id)::text AS \"key!\", min(a.email::text) AS label, count(*) AS \"requests!\", \
             count(*) FILTER (WHERE (u.prompt_tokens IS NOT NULL OR u.completion_tokens IS NOT NULL) AND NOT coalesce(u.estimated, false)) AS \"reported!\", \
             count(*) FILTER (WHERE u.estimated) AS \"estimated!\", \
             coalesce(sum(u.prompt_tokens),0)::bigint AS \"prompt_tokens!\", \
             coalesce(sum(u.completion_tokens),0)::bigint AS \"completion_tokens!\", \
             sum(u.cached_tokens)::bigint AS cached_tokens, sum(u.reasoning_tokens)::bigint AS reasoning_tokens \
             FROM usage u LEFT JOIN conversations c ON c.id = u.conversation_id \
             LEFT JOIN users a ON a.id = COALESCE(u.user_id, c.owner_id) \
             WHERE u.created_at >= $1 AND u.created_at < $2 GROUP BY COALESCE(u.user_id, c.owner_id) \
             ORDER BY coalesce(sum(u.prompt_tokens),0) + coalesce(sum(u.completion_tokens),0) DESC, \
             COALESCE(u.user_id, c.owner_id)::text \
             LIMIT $3",
            start,
            end,
            limit
        )
        .fetch_all(pool)
        .await?,
        Group::Route => sqlx::query_as!(
            Row,
            "SELECT u.route AS \"key!\", min(r.display_name) AS label, count(*) AS \"requests!\", \
             count(*) FILTER (WHERE (u.prompt_tokens IS NOT NULL OR u.completion_tokens IS NOT NULL) AND NOT coalesce(u.estimated, false)) AS \"reported!\", \
             count(*) FILTER (WHERE u.estimated) AS \"estimated!\", \
             coalesce(sum(u.prompt_tokens),0)::bigint AS \"prompt_tokens!\", \
             coalesce(sum(u.completion_tokens),0)::bigint AS \"completion_tokens!\", \
             sum(u.cached_tokens)::bigint AS cached_tokens, sum(u.reasoning_tokens)::bigint AS reasoning_tokens \
             FROM usage u LEFT JOIN model_routes r ON r.name = u.route \
             WHERE u.created_at >= $1 AND u.created_at < $2 GROUP BY u.route \
             ORDER BY coalesce(sum(u.prompt_tokens),0) + coalesce(sum(u.completion_tokens),0) DESC, u.route \
             LIMIT $3",
            start,
            end,
            limit
        )
        .fetch_all(pool)
        .await?,
    };
    Ok(rows)
}

#[cfg(test)]
#[path = "usage_tests.rs"]
mod tests;
