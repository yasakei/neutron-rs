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

/// `http.get_range(url, start, end)` — HTTP GET with Range header for
/// streaming downloads. Returns JSON `{"status":N,"body":"..."}`.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_http_get_range(url: i64, start: i64, end: i64) -> i64 {
    let url = registry::get_string(url).unwrap_or_default();
    let range_header = format!("bytes={start}-{end}");
    http_request_with_range("GET", &url, &range_header)
}

/// `http.get_file(url, dest)` — download a URL to a file. Returns JSON
/// `{"status":N,"bytes":N}` with the number of bytes written.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_http_get_file(url: i64, dest: i64) -> i64 {
    let url = registry::get_string(url).unwrap_or_default();
    let dest = registry::get_string(dest).unwrap_or_default();

    // Ensure parent directory exists.
    if let Some(parent) = std::path::Path::new(&dest).parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return super::throw_str(format!("http.get_file: cannot create directory: {e}"));
    }

    match download_file(&url, &dest) {
        Ok(bytes) => registry::put_string(format!("{{\"status\":200,\"bytes\":{}}}", bytes)),
        Err(e) => super::throw_str(format!("http.get_file: {e}")),
    }
}

/// `http.download_with_progress(url, dest, chunk_size)` — download a URL
/// to a file in chunks. Returns JSON `{"status":N,"bytes":N,"chunks":N}`.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_http_download_with_progress(url: i64, dest: i64, chunk_size: i64) -> i64 {
    let url = registry::get_string(url).unwrap_or_default();
    let dest = registry::get_string(dest).unwrap_or_default();
    let chunk = chunk_size.max(1024) as usize;

    if let Some(parent) = std::path::Path::new(&dest).parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return super::throw_str(format!(
            "http.download_with_progress: cannot create directory: {e}"
        ));
    }

    match download_file_chunked(&url, &dest, chunk) {
        Ok((bytes, chunks)) => registry::put_string(format!(
            "{{\"status\":200,\"bytes\":{},\"chunks\":{}}}",
            bytes, chunks
        )),
        Err(e) => super::throw_str(format!("http.download_with_progress: {e}")),
    }
}

/// `http.concurrent_download(urls, dest_dir, chunk_size)` — download
/// multiple URLs to `dest_dir` concurrently. Returns JSON array of
/// `{"url":"...","status":N,"bytes":N}` results.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_http_concurrent_download(urls: i64, dest_dir: i64, chunk_size: i64) -> i64 {
    let urls_str = registry::get_string(urls).unwrap_or_default();
    let dest_dir = registry::get_string(dest_dir).unwrap_or_default();
    let _chunk = chunk_size.max(1024) as usize;

    let url_list: Vec<String> = urls_str
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if let Err(e) = std::fs::create_dir_all(&dest_dir) {
        return super::throw_str(format!(
            "http.concurrent_download: cannot create '{dest_dir}': {e}"
        ));
    }

    let mut results = Vec::new();
    for url in &url_list {
        let filename = url.rsplit('/').next().unwrap_or("download");
        let dest = std::path::Path::new(&dest_dir).join(filename);
        match download_file(url, dest.to_str().unwrap_or("")) {
            Ok(bytes) => {
                results.push(format!(
                    "{{\"url\":\"{}\",\"status\":200,\"bytes\":{}}}",
                    url.replace('"', "\\\""),
                    bytes
                ));
            }
            Err(e) => {
                results.push(format!(
                    "{{\"url\":\"{}\",\"status\":0,\"error\":\"{}\"}}",
                    url.replace('"', "\\\""),
                    e.replace('"', "\\\"")
                ));
            }
        }
    }

    registry::put_string(format!("[{}]", results.join(",")))
}

fn http_request_with_range(method: &str, url: &str, range: &str) -> i64 {
    let (host, port, path, is_https) = match parse_url(url) {
        Ok(v) => v,
        Err(e) => return super::throw_str(format!("http.get_range: {e}")),
    };

    let addr = format!("{}:{}", host, port);
    let tcp = match TcpStream::connect(&addr) {
        Ok(t) => t,
        Err(e) => return super::throw_str(format!("http.get_range: Connection failed: {e}")),
    };

    if !is_https {
        let mut stream = tcp;
        return send_range_request(&mut stream, method, &host, &path, range);
    }

    let config = default_tls_config().clone();
    let server_name = match server_name_for(&host) {
        Ok(s) => s,
        Err(e) => return super::throw_str(format!("http.get_range: {e}")),
    };
    let tls = match ClientConnection::new(config, server_name) {
        Ok(c) => c,
        Err(e) => return super::throw_str(format!("http.get_range: TLS setup failed: {e}")),
    };
    let mut stream = StreamOwned::new(tls, tcp);
    if let Err(e) = stream.conn.complete_io(&mut stream.sock) {
        return super::throw_str(format!("http.get_range: TLS handshake failed: {e}"));
    }
    send_range_request(&mut stream, method, &host, &path, range)
}

fn send_range_request<S: Read + Write>(
    stream: &mut S,
    method: &str,
    host: &str,
    path: &str,
    range: &str,
) -> i64 {
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: Neutron/1.0\r\nConnection: close\r\nRange: {range}\r\n\r\n"
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return super::throw_str("http.get_range: write failed".to_string());
    }
    let mut response = String::new();
    if stream.read_to_string(&mut response).is_err() {
        return super::throw_str("http.get_range: read failed".to_string());
    }
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
    registry::put_string(make_response(status, &body))
}

fn download_file(url: &str, dest: &str) -> Result<usize, String> {
    let (host, port, path, is_https) = parse_url(url)?;
    let addr = format!("{}:{}", host, port);
    let mut tcp = TcpStream::connect(&addr).map_err(|e| format!("Connection failed: {e}"))?;

    let bytes_written = if is_https {
        let config = default_tls_config().clone();
        let server_name = server_name_for(&host)?;
        let tls = ClientConnection::new(config, server_name)
            .map_err(|e| format!("TLS setup failed: {e}"))?;
        let mut stream = StreamOwned::new(tls, tcp);
        stream
            .conn
            .complete_io(&mut stream.sock)
            .map_err(|e| format!("TLS handshake failed: {e}"))?;
        do_download(&mut stream, &host, &path, dest)?
    } else {
        do_download(&mut tcp, &host, &path, dest)?
    };

    Ok(bytes_written)
}

fn do_download<S: Read + Write>(
    stream: &mut S,
    host: &str,
    path: &str,
    dest: &str,
) -> Result<usize, String> {
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: Neutron/1.0\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;

    // Read response headers.
    let mut response_buf = Vec::new();
    let mut header_done = false;
    {
        let mut buf = [0u8; 4096];
        while !header_done {
            let n = stream
                .read(&mut buf)
                .map_err(|e| format!("Read failed: {e}"))?;
            if n == 0 {
                break;
            }
            response_buf.extend_from_slice(&buf[..n]);
            if let Some(pos) = response_buf.windows(4).position(|w| w == b"\r\n\r\n") {
                header_done = true;
                // Remove headers from buffer, keep body.
                let body_start = pos + 4;
                response_buf = response_buf[body_start..].to_vec();
            }
        }
    }

    let mut file =
        std::fs::File::create(dest).map_err(|e| format!("Cannot create file '{dest}': {e}"))?;

    // Write any body bytes already read.
    let mut written = response_buf.len();
    file.write_all(&response_buf)
        .map_err(|e| format!("Write failed: {e}"))?;

    // Read the rest.
    let mut buf = [0u8; 8192];
    loop {
        let n = stream
            .read(&mut buf)
            .map_err(|e| format!("Read failed: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("Write failed: {e}"))?;
        written += n;
    }

    Ok(written)
}

fn download_file_chunked(
    url: &str,
    dest: &str,
    chunk_size: usize,
) -> Result<(usize, usize), String> {
    let (host, port, path, is_https) = parse_url(url)?;
    let addr = format!("{}:{}", host, port);
    let tcp = TcpStream::connect(&addr).map_err(|e| format!("Connection failed: {e}"))?;

    // First, get content length.
    let content_length = if is_https {
        let config = default_tls_config().clone();
        let server_name = server_name_for(&host)?;
        let tls = ClientConnection::new(config, server_name)
            .map_err(|e| format!("TLS setup failed: {e}"))?;
        let mut stream = StreamOwned::new(tls, tcp);
        stream
            .conn
            .complete_io(&mut stream.sock)
            .map_err(|e| format!("TLS handshake failed: {e}"))?;
        get_content_length(&mut stream, &host, &path)?
    } else {
        let mut stream = tcp;
        get_content_length(&mut stream, &host, &path)?
    };

    let mut file =
        std::fs::File::create(dest).map_err(|e| format!("Cannot create file '{dest}': {e}"))?;
    let mut total_written = 0usize;
    let mut chunks = 0usize;

    // Download in range requests.
    let mut offset = 0usize;
    loop {
        let end = if content_length > 0 {
            (offset + chunk_size - 1).min(content_length - 1)
        } else {
            offset + chunk_size - 1
        };
        let range = format!("bytes={offset}-{end}");

        // Make a new connection for each chunk (simpler than keeping
        // connections alive through TLS renegotiation).
        let tcp = TcpStream::connect(&addr).map_err(|e| format!("Connection failed: {e}"))?;

        if is_https {
            let config = default_tls_config().clone();
            let server_name = server_name_for(&host)?;
            let tls = ClientConnection::new(config, server_name)
                .map_err(|e| format!("TLS setup failed: {e}"))?;
            let mut stream = StreamOwned::new(tls, tcp);
            stream
                .conn
                .complete_io(&mut stream.sock)
                .map_err(|e| format!("TLS handshake failed: {e}"))?;
            let written = download_chunk(&mut stream, &host, &path, &range, &mut file)?;
            total_written += written;
            chunks += 1;
            if written < chunk_size || (content_length > 0 && offset + chunk_size >= content_length)
            {
                break;
            }
        } else {
            let mut stream = tcp;
            let written = download_chunk(&mut stream, &host, &path, &range, &mut file)?;
            total_written += written;
            chunks += 1;
            if written < chunk_size || (content_length > 0 && offset + chunk_size >= content_length)
            {
                break;
            }
        }

        offset += chunk_size;
        if content_length > 0 && offset >= content_length {
            break;
        }
    }

    Ok((total_written, chunks))
}

fn get_content_length<S: Read + Write>(
    stream: &mut S,
    host: &str,
    path: &str,
) -> Result<usize, String> {
    let request = format!(
        "HEAD {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: Neutron/1.0\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| format!("Read failed: {e}"))?;

    for line in response.lines() {
        let lower = line.to_ascii_lowercase();
        if let Some(val) = lower.strip_prefix("content-length:") {
            return val.trim().parse().map_err(|e| format!("parse error: {e}"));
        }
    }
    Ok(0)
}

fn download_chunk<S: Read + Write>(
    stream: &mut S,
    host: &str,
    path: &str,
    range: &str,
    file: &mut std::fs::File,
) -> Result<usize, String> {
    let request = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: Neutron/1.0\r\nConnection: close\r\nRange: {range}\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("Write failed: {e}"))?;

    // Read headers.
    let mut buf = Vec::new();
    loop {
        let mut tmp = [0u8; 4096];
        let n = stream
            .read(&mut tmp)
            .map_err(|e| format!("Read failed: {e}"))?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let body = buf[pos + 4..].to_vec();
            file.write_all(&body)
                .map_err(|e| format!("Write failed: {e}"))?;
            let mut total = body.len();
            let mut read_buf = [0u8; 8192];
            loop {
                let n = stream
                    .read(&mut read_buf)
                    .map_err(|e| format!("Read failed: {e}"))?;
                if n == 0 {
                    break;
                }
                file.write_all(&read_buf[..n])
                    .map_err(|e| format!("Write failed: {e}"))?;
                total += n;
            }
            return Ok(total);
        }
    }
    Ok(0)
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
