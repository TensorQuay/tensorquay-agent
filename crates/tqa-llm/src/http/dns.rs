//! Retain the DNS phase even when its OS error resembles a refused model connection.
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::{error::Error, sync::Arc};

#[derive(Debug, thiserror::Error)]
#[error("DNS resolution failed")]
pub(super) struct Failure(#[source] Box<dyn Error + Send + Sync>);

pub(super) struct Resolver(pub(super) Arc<dyn Resolve>);
impl Resolve for Resolver {
    fn resolve(&self, name: Name) -> Resolving {
        let pending = self.0.resolve(name);
        Box::pin(async move {
            pending
                .await
                .map_err(|error| Box::new(Failure(error)) as Box<dyn Error + Send + Sync>)
        })
    }
}

pub(crate) struct SystemDns;
impl Resolve for SystemDns {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let addresses = tokio::net::lookup_host((name.as_str(), 0)).await?;
            Ok(Box::new(addresses.collect::<Vec<_>>().into_iter()) as Addrs)
        })
    }
}
