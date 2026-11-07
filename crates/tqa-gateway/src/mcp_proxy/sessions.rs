//! MCP sessions through the proxy (ADR-0031 decision 4). The remote's `Mcp-Session-Id` never reaches a sandbox: the
//! runtime gets an opaque id bound to its conversation, the connector, the remote id and the credential source that
//! opened the session. Another conversation, another connector, an idle session or a changed credential source finds
//! nothing, which MCP clients answer by initialising again; so a session never outlives the credential that opened it.
//! Pure, with time passed in (GW-U-042).
use std::{collections::HashMap, time::Duration};

use tokio::time::Instant;

use crate::connectors::CredentialSource;

/// Sessions one conversation may hold at once; opening another closes its least recently used one.
pub(crate) const MAX_SESSIONS_PER_CONVERSATION: usize = 8;
/// A session unused this long is forgotten.
pub(crate) const SESSION_IDLE: Duration = Duration::from_mins(10);

struct Session {
    conversation: uuid::Uuid,
    connector: String,
    remote: String,
    source: Option<CredentialSource>,
    last_used: Instant,
    streaming: bool,
}

/// What a request's session id resolved to.
#[derive(Debug, Eq, PartialEq)]
pub(crate) enum Found {
    /// The remote session to address.
    Remote(String),
    /// Nothing usable: unknown, expired, another conversation's or connector's, or opened with another credential.
    Unknown,
}

/// The live sessions, by opaque id.
#[derive(Default)]
pub(crate) struct Sessions {
    map: HashMap<String, Session>,
}

impl Sessions {
    /// Records a session the remote opened; returns the opaque id for the runtime. Idle sessions are forgotten first,
    /// and a conversation at its bound loses its least recently used session.
    pub(crate) fn open(
        &mut self,
        (id, remote): (String, String),
        (conversation, connector): (uuid::Uuid, &str),
        source: Option<CredentialSource>,
        now: Instant,
    ) -> String {
        self.expire(now);
        let held: Vec<(&String, Instant)> = self
            .map
            .iter()
            .filter(|(_, session)| session.conversation == conversation)
            .map(|(id, session)| (id, session.last_used))
            .collect();
        if held.len() >= MAX_SESSIONS_PER_CONVERSATION
            && let Some(oldest) = held
                .iter()
                .min_by_key(|(_, used)| *used)
                .map(|(id, _)| (*id).clone())
        {
            self.map.remove(&oldest);
        }
        self.map.insert(
            id.clone(),
            Session {
                conversation,
                connector: connector.to_owned(),
                remote,
                source,
                last_used: now,
                streaming: false,
            },
        );
        id
    }

    /// The remote session for a request, refreshing its use; anything unusable is forgotten and found as unknown.
    pub(crate) fn find(
        &mut self,
        id: &str,
        (conversation, connector): (uuid::Uuid, &str),
        source: Option<CredentialSource>,
        now: Instant,
    ) -> Found {
        self.expire(now);
        let Some(session) = self.map.get_mut(id) else {
            return Found::Unknown;
        };
        if session.conversation != conversation || session.connector != connector {
            // Someone else's id: refuse without touching their session.
            return Found::Unknown;
        }
        if session.source != source {
            self.map.remove(id);
            return Found::Unknown;
        }
        session.last_used = now;
        Found::Remote(session.remote.clone())
    }

    /// Forgets the caller's session and returns its remote id, for a DELETE.
    pub(crate) fn close(
        &mut self,
        id: &str,
        (conversation, connector): (uuid::Uuid, &str),
    ) -> Found {
        match self.map.get(id) {
            Some(session)
                if session.conversation == conversation && session.connector == connector =>
            {
                self.map
                    .remove(id)
                    .map_or(Found::Unknown, |session| Found::Remote(session.remote))
            }
            _ => Found::Unknown,
        }
    }

    /// Marks the session's server stream open; `false` when one already is (one stream per session).
    pub(crate) fn start_stream(&mut self, id: &str) -> bool {
        match self.map.get_mut(id) {
            Some(session) if !session.streaming => {
                session.streaming = true;
                true
            }
            _ => false,
        }
    }

    /// The session's server stream ended.
    pub(crate) fn end_stream(&mut self, id: &str) {
        if let Some(session) = self.map.get_mut(id) {
            session.streaming = false;
        }
    }

    /// Forgets sessions idle longer than [`SESSION_IDLE`]; a session with an open stream is in use.
    fn expire(&mut self, now: Instant) {
        self.map.retain(|_, session| {
            session.streaming || now.saturating_duration_since(session.last_used) < SESSION_IDLE
        });
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.len()
    }
}

#[cfg(test)]
#[path = "sessions_tests.rs"]
mod tests;
