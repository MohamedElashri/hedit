//! Only tests construct local endpoints and trust the public fixture CA.
use super::*;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_rustls::{
    rustls::{
        self,
        pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
    },
    TlsAcceptor,
};
const CERT: &[u8] = include_bytes!("../../tests/fixtures/tls-cert.der");
const ROOT: &[u8] = include_bytes!("../../tests/fixtures/tls-root.der");
const KEY: &[u8] = include_bytes!("../../tests/fixtures/tls-key.der");
async fn server(response: &'static [u8], stall: bool) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(CERT.to_vec())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(KEY.to_vec())),
        )
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let task = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let Ok(mut stream) = acceptor.accept(stream).await else {
            return;
        };
        let mut bytes = vec![0u8; 4096];
        let mut used = 0;
        loop {
            let Ok(n) = stream.read(&mut bytes[used..]).await else {
                return;
            };
            if n == 0 {
                return;
            }
            used += n;
            if bytes[..used].windows(4).any(|part| part == b"\r\n\r\n") {
                break;
            }
            if used == bytes.len() {
                return;
            }
        }
        let request = std::str::from_utf8(&bytes[..used]).unwrap();
        assert!(request.starts_with("POST /translate HTTP/1.1\r\n"));
        assert!(request
            .to_ascii_lowercase()
            .contains("accept-encoding: identity"));
        assert!(request
            .to_ascii_lowercase()
            .contains("content-type: application/json"));
        assert!(!request.to_ascii_lowercase().contains("authorization:"));
        if stall {
            let mut byte = [0];
            let _ = stream.read(&mut byte).await;
            let _ = stream.read(&mut byte).await;
            return;
        }
        let _ = stream.write_all(response).await;
        let _ = stream.shutdown().await;
    });
    (address, task)
}
fn local_endpoint(address: SocketAddr, maximum: usize) -> Endpoint {
    Endpoint {
        url: Url::parse(&format!(
            "https://example.test:{}/translate",
            address.port()
        ))
        .unwrap(),
        method: Method::POST,
        upload: 64,
        response: maximum,
        content_type: ContentType::Json,
    }
}
fn client(address: SocketAddr, trusted: bool) -> Client {
    let mut builder = Client::builder()
        .no_proxy()
        .https_only(true)
        .http1_only()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(2))
        .resolve_to_addrs("example.test", &[address]);
    if trusted {
        builder = builder.add_root_certificate(reqwest::Certificate::from_der(ROOT).unwrap());
    }
    builder.build().unwrap()
}
#[tokio::test]
async fn tls_transport_pins_addresses_checks_trust_and_bounds_encoded_and_streamed_bodies() {
    for (reply,maximum,accepted) in [
        (b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nsafe".as_slice(),4,true),
        (b"HTTP/1.1 302 Found\r\nLocation: https://127.0.0.1/private\r\nContent-Length: 0\r\n\r\n".as_slice(),64,false),
        (b"HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: 4\r\n\r\nsafe".as_slice(),64,false),
        (b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nlarge".as_slice(),4,false),
        (b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\none\r\n3\r\ntwo\r\n0\r\n\r\n".as_slice(),4,false),
        (b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n\xff".as_slice(),64,false),
    ] {
        let (address,server)=server(reply,false).await;let endpoint=local_endpoint(address,maximum);let result=transfer(&endpoint,"text".into(),&[address],&client(address,true)).await;assert_eq!(result.is_ok(),accepted,"{result:?}");server.await.unwrap();
    }
    let (address, server) = server(b"", false).await;
    let endpoint = local_endpoint(address, 64);
    assert!(transfer(
        &endpoint,
        "text".into(),
        &[address],
        &client(address, false)
    )
    .await
    .is_err());
    server.await.unwrap();
}
#[tokio::test]
async fn a_stalled_transport_can_be_dropped_immediately_without_waiting_for_the_server() {
    let (address, server) = server(b"", true).await;
    let endpoint = local_endpoint(address, 64);
    let cancel = Cancel::default();
    let client = client(address, true);
    let addresses = [address];
    let mut request = Box::pin(transfer(&endpoint, "fixture".into(), &addresses, &client));
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut request)
            .await
            .is_err()
    );
    cancel.cancel();
    let start = Instant::now();
    tokio::select! {biased;_=cancel.cancelled()=>{},_=&mut request=>panic!("stalled response unexpectedly finished")};
    assert!(start.elapsed() < Duration::from_millis(100));
    drop(request);
    drop(client);
    tokio::time::timeout(Duration::from_secs(1), server)
        .await
        .unwrap()
        .unwrap();
}
