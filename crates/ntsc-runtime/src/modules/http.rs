//! NTSC standard library: `http` module.
//! Blocking HTTP/1.1 client; `https://` uses rustls anchored at Mozilla's
//! bundled webpki roots. Responses are JSON: `{"status":N,"body":"..."}`.

use std::io::{Read, Write};
use std::net::{IpAddr, TcpStream};
use std::sync::{Arc, OnceLock};

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

use crate::registry;

fn default_tls_config() -> &'static Arc<ClientConfig> {
    static DEFAULT_TLS: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    DEFAULT_TLS.get_or_init(|| {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        )
    })
}

fn make_response(status: i64, body: &str) -> String {
    format!(
        "{{\"status\":{},\"body\":\"{}\"}}",
        status,
        body.replace('"', "\\\"").replace('\n', "\\n")
    )
}

fn parse_url(url: &str) -> Result<(String, u16, String, bool), String> {
    let is_https = url.starts_with("https://");
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);

    let default_port = if is_https { 443 } else { 80 };

    let (host_part, path) = match rest.find('/') {
        Some(pos) => (&rest[..pos], &rest[pos..]),
        None => (rest, "/"),
    };

    let (host_str, port) = if let Some(colon_pos) = host_part.rfind(':') {
        let h = &host_part[..colon_pos];
        let p: u16 = host_part[colon_pos + 1..]
            .parse()
            .map_err(|_| "Invalid port".to_string())?;
        (h.to_string(), p)
    } else {
        (host_part.to_string(), default_port)
    };

    Ok((host_str, port, path.to_string(), is_https))
}

fn server_name_for(host: &str) -> Result<ServerName<'static>, String> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(ServerName::IpAddress(ip.into()));
    }
    ServerName::try_from(host.to_string()).map_err(|_| format!("invalid host `{host}`"))
}

fn http_request_raw(method: &str, url: &str, body: Option<&str>) -> Result<(i64, String), String> {
    http_request_raw_impl(method, url, body, None)
}

#[cfg(test)]
fn http_request_raw_with_roots(
    method: &str,
    url: &str,
    body: Option<&str>,
    roots: &RootCertStore,
) -> Result<(i64, String), String> {
    http_request_raw_impl(method, url, body, Some(roots))
}

fn http_request_raw_impl(
    method: &str,
    url: &str,
    body: Option<&str>,
    roots: Option<&RootCertStore>,
) -> Result<(i64, String), String> {
    let (host, port, path, is_https) = parse_url(url)?;

    let addr = format!("{}:{}", host, port);
    let tcp = TcpStream::connect(&addr).map_err(|e| format!("Connection failed: {e}"))?;

    if !is_https {
        let mut stream = tcp;
        return send_request(&mut stream, method, &host, &path, body);
    }

    let config = match roots {
        Some(roots) => Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots.clone())
                .with_no_client_auth(),
        ),
        None => default_tls_config().clone(),
    };
    let server_name = server_name_for(&host)?;
    let tls =
        ClientConnection::new(config, server_name).map_err(|e| format!("TLS setup failed: {e}"))?;
    let mut stream = StreamOwned::new(tls, tcp);

    // Drive the handshake before sending so certificate-validation failures
    // surface as a clean connection error.
    stream
        .conn
        .complete_io(&mut stream.sock)
        .map_err(|e| format!("TLS handshake failed: {e}"))?;
    send_request(&mut stream, method, &host, &path, body)
}

fn send_request<S: Read + Write>(
    stream: &mut S,
    method: &str,
    host: &str,
    path: &str,
    body: Option<&str>,
) -> Result<(i64, String), String> {
    let mut request = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Neutron/1.0\r\nConnection: close\r\n",
        method, path, host
    );

    if let Some(data) = body {
        request.push_str("Content-Type: application/octet-stream\r\n");
        request.push_str(&format!("Content-Length: {}\r\n", data.len()));
    }
    request.push_str("\r\n");

    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;

    if let Some(data) = body {
        stream
            .write_all(data.as_bytes())
            .map_err(|e| format!("Body write failed: {e}"))?;
    }

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| format!("Read failed: {e}"))?;

    let status = if let Some(line) = response.lines().next() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            parts[1].parse::<i64>().unwrap_or(0)
        } else {
            0
        }
    } else {
        0
    };

    let body = if let Some(pos) = response.find("\r\n\r\n") {
        response[pos + 4..].to_string()
    } else {
        String::new()
    };

    Ok((status, body))
}

/// `http.get(url)` — returns the response JSON; throws when the request
/// cannot be completed.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_http_get(url: i64) -> i64 {
    let url = registry::get_string(url).unwrap_or_default();
    match http_request_raw("GET", &url, None) {
        Ok((status, body)) => registry::put_string(make_response(status, &body)),
        Err(e) => super::throw_str(format!("http.get: {e}")),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_http_post(url: i64, data: i64) -> i64 {
    let url = registry::get_string(url).unwrap_or_default();
    let data = registry::get_string(data).unwrap_or_default();
    match http_request_raw("POST", &url, Some(&data)) {
        Ok((status, body)) => registry::put_string(make_response(status, &body)),
        Err(e) => super::throw_str(format!("http.post: {e}")),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_http_put(url: i64, data: i64) -> i64 {
    let url = registry::get_string(url).unwrap_or_default();
    let data = registry::get_string(data).unwrap_or_default();
    match http_request_raw("PUT", &url, Some(&data)) {
        Ok((status, body)) => registry::put_string(make_response(status, &body)),
        Err(e) => super::throw_str(format!("http.put: {e}")),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_http_delete(url: i64) -> i64 {
    let url = registry::get_string(url).unwrap_or_default();
    match http_request_raw("DELETE", &url, None) {
        Ok((status, body)) => registry::put_string(make_response(status, &body)),
        Err(e) => super::throw_str(format!("http.delete: {e}")),
    }
}

/// `http.head(url)` — like `get`, but the JSON carries only `status`.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_http_head(url: i64) -> i64 {
    let url = registry::get_string(url).unwrap_or_default();
    match http_request_raw("HEAD", &url, None) {
        Ok((status, _body)) => registry::put_string(format!("{{\"status\":{}}}", status)),
        Err(e) => super::throw_str(format!("http.head: {e}")),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_http_patch(url: i64, data: i64) -> i64 {
    let url = registry::get_string(url).unwrap_or_default();
    let data = registry::get_string(data).unwrap_or_default();
    match http_request_raw("PATCH", &url, Some(&data)) {
        Ok((status, body)) => registry::put_string(make_response(status, &body)),
        Err(e) => super::throw_str(format!("http.patch: {e}")),
    }
}

/// `http.request(method, url, data)` — an empty `data` sends no body.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_http_request(method: i64, url: i64, data: i64) -> i64 {
    let method = registry::get_string(method).unwrap_or_default();
    let url = registry::get_string(url).unwrap_or_default();
    let data_str = registry::get_string(data).unwrap_or_default();
    let body = if data_str.is_empty() {
        None
    } else {
        Some(data_str.as_str())
    };
    match http_request_raw(&method, &url, body) {
        Ok((status, body)) => registry::put_string(make_response(status, &body)),
        Err(e) => super::throw_str(format!("http.request: {e}")),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_http_status_code(response: i64) -> i64 {
    let resp = registry::get_string(response).unwrap_or_default();

    if let Some(pos) = resp.find("\"status\":") {
        let rest = &resp[pos + 9..];
        let num_end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '-')
            .unwrap_or(rest.len());
        rest[..num_end].parse::<i64>().unwrap_or(0)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::CertifiedKey;
    use rustls::pki_types::PrivateKeyDer;
    use rustls::{ServerConfig, ServerConnection};
    use std::net::TcpListener;
    use std::thread;

    fn read_http_request<S: Read>(stream: &mut S) -> (String, Vec<u8>) {
        let mut data = Vec::new();
        let mut buf = [0u8; 1024];
        let header_len = loop {
            if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                break pos + 4;
            }
            let n = stream.read(&mut buf).unwrap_or(0);
            if n == 0 {
                break data.len();
            }
            data.extend_from_slice(&buf[..n]);
        };
        let head = String::from_utf8_lossy(&data[..header_len]).to_string();
        let mut body = data[header_len..].to_vec();
        let declared: usize = head
            .lines()
            .find_map(|l| {
                let lower = l.to_ascii_lowercase();
                lower
                    .strip_prefix("content-length:")
                    .and_then(|v| v.trim().parse().ok())
            })
            .unwrap_or(0);
        while body.len() < declared {
            let n = stream.read(&mut buf).unwrap_or(0);
            if n == 0 {
                break;
            }
            body.extend_from_slice(&buf[..n]);
        }
        (head, body)
    }

    fn spawn_plain_server() -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let (head, _body) = read_http_request(&mut stream);
            assert!(head.starts_with("GET /greet HTTP/1.1"));
            let response = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
            stream.write_all(response.as_bytes()).unwrap();
        });
        (format!("http://127.0.0.1:{port}/greet"), handle)
    }

    fn spawn_tls_server() -> (String, RootCertStore, thread::JoinHandle<()>) {
        let CertifiedKey {
            cert, signing_key, ..
        } = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
        let cert_der = cert.der().clone();
        let key_der = PrivateKeyDer::Pkcs8(signing_key.serialize_der().into());
        let server_config = Arc::new(
            ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![cert_der.clone()], key_der)
                .unwrap(),
        );

        let mut roots = RootCertStore::empty();
        roots.add(cert_der).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (tcp, _) = listener.accept().unwrap();
            let conn = ServerConnection::new(server_config).unwrap();
            let mut stream = StreamOwned::new(conn, tcp);

            let (head, _body) = read_http_request(&mut stream);
            if head.is_empty() {
                return;
            }
            assert!(head.starts_with("GET /secure HTTP/1.1"));
            let response =
                "HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nsecure!";
            stream.write_all(response.as_bytes()).unwrap();

            stream.conn.send_close_notify();
            // rustls treats a bare socket EOF without close_notify as a
            // truncation attack, so shut down explicitly.
            let _ = stream.flush();
        });
        (format!("https://localhost:{port}/secure"), roots, handle)
    }

    fn put(s: &str) -> i64 {
        registry::put_string(s.to_string())
    }

    fn read(id: i64) -> String {
        let s = registry::get_string(id).unwrap_or_default();
        let _ = registry::take_string(id);
        s
    }

    #[test]
    fn test_parse_url() {
        let (host, port, path, is_https) = parse_url("http://example.com/path").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 80);
        assert_eq!(path, "/path");
        assert!(!is_https);
    }

    #[test]
    fn test_parse_url_with_port() {
        let (host, port, path, is_https) = parse_url("http://localhost:8080/api").unwrap();
        assert_eq!(host, "localhost");
        assert_eq!(port, 8080);
        assert_eq!(path, "/api");
        assert!(!is_https);
    }

    #[test]
    fn test_parse_https_url() {
        let (host, port, path, is_https) = parse_url("https://example.com/secure").unwrap();
        assert_eq!(host, "example.com");
        assert_eq!(port, 443);
        assert_eq!(path, "/secure");
        assert!(is_https);
    }

    #[test]
    fn plain_http_request_round_trip() {
        let (url, server) = spawn_plain_server();
        let (status, body) = http_request_raw("GET", &url, None).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, "ok");
        server.join().unwrap();
    }

    #[test]
    fn http_post_sends_body() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let (head, body) = read_http_request(&mut stream);
            assert!(head.starts_with("POST /submit HTTP/1.1"));
            assert_eq!(body, b"hello world");
            let response =
                "HTTP/1.1 201 Created\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
            stream.write_all(response.as_bytes()).unwrap();
        });
        let url = format!("http://127.0.0.1:{port}/submit");
        let (status, body) = http_request_raw("POST", &url, Some("hello world")).unwrap();
        assert_eq!(status, 201);
        assert_eq!(body, "ok");
        handle.join().unwrap();
    }

    #[test]
    fn https_request_round_trip_uses_tls() {
        let (url, roots, server) = spawn_tls_server();
        let (status, body) = http_request_raw_with_roots("GET", &url, None, &roots).unwrap();
        assert_eq!(status, 200);
        assert_eq!(body, "secure!");
        server.join().unwrap();
    }

    #[test]
    fn https_request_rejects_untrusted_cert() {
        let (url, _roots, server) = spawn_tls_server();
        let err = http_request_raw("GET", &url, None).unwrap_err();
        assert!(
            err.contains("TLS handshake failed"),
            "expected a TLS handshake failure, got: {err}"
        );
        server.join().unwrap();
    }

    #[test]
    fn http_get_round_trips_via_handle_abi() {
        let (url, server) = spawn_plain_server();
        let response = ntsc_http_get(put(&url));
        assert_eq!(read(response), "{\"status\":200,\"body\":\"ok\"}");
        server.join().unwrap();
    }

    #[test]
    fn status_code_extracts_from_response_json() {
        assert_eq!(
            ntsc_http_status_code(put("{\"status\":404,\"body\":\"not found\"}")),
            404
        );
    }

    #[test]
    fn http_get_failure_throws() {
        use crate::modules::test_util::catch_throw;
        let url = put("http://127.0.0.1:1/nothing");
        let err = catch_throw(|| {
            let _ = ntsc_http_get(url);
        });
        let msg = err.unwrap();
        assert!(msg.contains("http.get"), "unexpected message: {msg}");
        let _ = registry::take_string(url);
    }
}
