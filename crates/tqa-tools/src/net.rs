//! The runtime tools' own TCP connections (git push's relay, `fetch_url` through the egress proxy) carry small writes
//! at once: `TCP_NODELAY` on each, as the gateway does on its side (TOOL-U-042).
use tokio::net::{TcpStream, ToSocketAddrs};

/// Opens a TCP connection with `TCP_NODELAY`; a refusal of the option is logged, the connection still used.
pub(crate) async fn connect(address: impl ToSocketAddrs) -> std::io::Result<TcpStream> {
    let stream = TcpStream::connect(address).await?;
    if let Err(error) = stream.set_nodelay(true) {
        tracing::warn!(%error, "could not set TCP_NODELAY on a connection");
    }
    Ok(stream)
}

#[cfg(test)]
mod tests {
    // TEST: TOOL-U-042
    #[tokio::test]
    async fn tool_u_042_a_tools_connection_has_nodelay() -> Result<(), Box<dyn std::error::Error>> {
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let opened = super::connect(upstream.local_addr()?).await?;
        assert!(opened.nodelay()?);
        Ok(())
    }
}
