use crate::GatewayError;
use axum::http::HeaderMap;
use serde::Deserialize;
use tqa_types::Seq;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EventQuery {
    #[serde(default, deserialize_with = "decimal_cursor")]
    pub after: Option<u64>,
    pub limit: Option<u32>,
    pub stream: Option<u8>,
    pub protocol: Option<u16>,
}

impl EventQuery {
    pub fn is_stream(&self) -> Result<bool, GatewayError> {
        match self.stream {
            None | Some(0) => Ok(false),
            Some(1) => Ok(true),
            Some(_) => Err(GatewayError::BadRequest("stream must be zero or one")),
        }
    }

    pub fn page(&self, maximum: u32) -> Result<(Seq, u32), GatewayError> {
        let after = self.after.unwrap_or(0).into();
        crate::wire::db_sequence(after)?;
        let limit = self.limit.unwrap_or(100);
        if !(1..=maximum).contains(&limit) {
            return Err(GatewayError::BadRequest("page limit is out of bounds"));
        }
        Ok((after, limit))
    }

    pub fn stream_start(&self, headers: &HeaderMap) -> Result<Seq, GatewayError> {
        let protocol = self
            .protocol
            .ok_or(GatewayError::BadRequest("stream protocol is required"))?;
        tqa_types::check_protocol(protocol).map_err(GatewayError::UnsupportedProtocol)?;
        let fallback = self.after.unwrap_or(0).into();
        crate::wire::db_sequence(fallback)?;
        if self.limit.is_some() {
            return Err(GatewayError::BadRequest(
                "stream replay does not accept a limit",
            ));
        }
        let mut values = headers.get_all("last-event-id").iter();
        let first = values.next();
        if values.next().is_some() {
            return Err(GatewayError::BadRequest("Last-Event-ID must appear once"));
        }
        let Some(value) = first else {
            return Ok(fallback);
        };
        let text = value
            .to_str()
            .map_err(|_| GatewayError::BadRequest("Last-Event-ID is invalid"))?;
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(GatewayError::BadRequest(
                "Last-Event-ID must be a decimal sequence",
            ));
        }
        let sequence = text
            .parse::<u64>()
            .map_err(|_| GatewayError::BadRequest("Last-Event-ID exceeds store limit"))?
            .into();
        crate::wire::db_sequence(sequence)?;
        Ok(sequence)
    }
}

fn decimal_cursor<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    let text = Option::<String>::deserialize(deserializer)?;
    text.map(|text| {
        if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(serde::de::Error::custom("after must be a decimal sequence"));
        }
        text.parse()
            .map_err(|_| serde::de::Error::custom("after exceeds store limit"))
    })
    .transpose()
}
