//! Real TLS on both sides of CONNECT, using an ephemeral fixture certificate.
use super::*;
use std::{error::Error, io, process::Stdio};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpListener,
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        ServerConfig,
        pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    },
};

type TestResult = Result<(), Box<dyn Error>>;
trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}
type Socket = Box<dyn Stream>;

struct Certificate {
    root: CertificateDer<'static>,
    server: Arc<ServerConfig>,
}
impl Certificate {
    async fn create() -> Result<Self, Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let config = directory.path().join("certificate.cnf");
        std::fs::write(
            &config,
            "[req]\nprompt=no\ndistinguished_name=subject\nx509_extensions=extensions\n[subject]\nCN=localhost\n[extensions]\nsubjectAltName=DNS:localhost\nbasicConstraints=critical,CA:FALSE\nextendedKeyUsage=serverAuth\nkeyUsage=critical,digitalSignature,keyEncipherment\n",
        )?;
        let key = directory.path().join("key.pem");
        let cert = directory.path().join("cert.pem");
        let mut command = tokio::process::Command::new("openssl");
        command
            .args([
                "req", "-x509", "-nodes", "-newkey", "rsa:2048", "-days", "1", "-config",
            ])
            .arg(config)
            .arg("-keyout")
            .arg(&key)
            .arg("-out")
            .arg(&cert)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        assert!(
            tokio::time::timeout(Duration::from_secs(30), command.status())
                .await??
                .success()
        );
        let root = CertificateDer::from_pem_file(cert)?;
        let key = PrivateKeyDer::from_pem_file(key)?;
        let server = ServerConfig::builder_with_provider(Arc::new(
            tokio_rustls::rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(vec![root.clone()], key)?;
        Ok(Self {
            root,
            server: Arc::new(server),
        })
    }
    fn trust(&self, tool: &mut FetchUrl) -> TestResult {
        let mut roots = RootCertStore::empty();
        roots.add(self.root.clone())?;
        let config = ClientConfig::builder_with_provider(Arc::new(
            tokio_rustls::rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
        tool.tls = Arc::new(config);
        Ok(())
    }
}

async fn head(stream: &mut Socket) -> io::Result<String> {
    let mut bytes = Vec::new();
    while bytes.len() < HEAD_BYTES {
        bytes.push(stream.read_u8().await?);
        if bytes.ends_with(b"\r\n\r\n") {
            return String::from_utf8(bytes).map_err(io::Error::other);
        }
    }
    Err(io::Error::other("fixture header bound"))
}
async fn serve(listener: TcpListener, tls: &Certificate, secure_proxy: bool) -> io::Result<()> {
    let (socket, _) = listener.accept().await?;
    let acceptor = TlsAcceptor::from(tls.server.clone());
    let mut socket: Socket = Box::new(socket);
    if secure_proxy {
        socket = Box::new(acceptor.accept(socket).await?);
    }
    let connect = head(&mut socket).await?.to_ascii_lowercase();
    assert!(connect.contains("proxy-authorization: bearer fixture-runtime-token"));
    socket
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    if !secure_proxy {
        socket = Box::new(acceptor.accept(socket).await?);
    }
    let origin = head(&mut socket).await?;
    assert!(origin.starts_with("GET /page HTTP/1.1"));
    assert!(!origin.to_ascii_lowercase().contains("authorization"));
    socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 8\r\nConnection: close\r\n\r\nTLS text").await?;
    socket.shutdown().await
}

#[tokio::test]
async fn tool_u_010_tls_verifies_both_proxy_and_destination() -> TestResult {
    let certificate = Certificate::create().await?;
    for secure_proxy in [false, true] {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let proxy_scheme = if secure_proxy { "https" } else { "http" };
        let origin_scheme = if secure_proxy { "http" } else { "https" };
        let endpoint = ProxyEndpoint::new(&format!(
            "{proxy_scheme}://localhost:{}",
            listener.local_addr()?.port()
        ))?;
        let mut tool = FetchUrl::new(endpoint, &"fixture-runtime-token".into())?;
        certificate.trust(&mut tool)?;
        let url = destination(&format!("{origin_scheme}://localhost/page"))?;
        let (response, server) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(
                tool.download(url),
                serve(listener, &certificate, secure_proxy)
            )
        })
        .await?;
        server?;
        assert_eq!(response?.bytes, b"TLS text");
    }
    Ok(())
}

#[tokio::test]
async fn tool_u_010_untrusted_tls_is_rejected_for_proxy_and_destination() -> TestResult {
    let certificate = Certificate::create().await?;
    for secure_proxy in [false, true] {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let proxy_scheme = if secure_proxy { "https" } else { "http" };
        let origin_scheme = if secure_proxy { "http" } else { "https" };
        let endpoint = ProxyEndpoint::new(&format!(
            "{proxy_scheme}://localhost:{}",
            listener.local_addr()?.port()
        ))?;
        let tool = FetchUrl::new(endpoint, &"fixture-runtime-token".into())?;
        let url = destination(&format!("{origin_scheme}://localhost/page"))?;
        let (response, server) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(
                tool.download(url),
                serve(listener, &certificate, secure_proxy)
            )
        })
        .await?;
        assert!(server.is_err());
        let error = response.err().ok_or("untrusted TLS succeeded")?;
        assert_eq!(error, ToolError::Io("TLS connection failed".into()));
    }
    Ok(())
}
