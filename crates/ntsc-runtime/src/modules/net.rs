//! NTSC standard library: `net` module.
//! TCP and UDP sockets, stored in the registry as opaque handles that must
//! be released with `net.close`.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};

use crate::registry;

/// Cap for `net.recv`/`net.udp_recv` buffers, in bytes.
const MAX_RECV_SIZE: usize = 64 * 1024 * 1024;

/// Tagged socket handle so every operation can validate the variant and
/// throw instead of misusing memory.
enum NetHandle {
    TcpStream(TcpStream),
    TcpListener(TcpListener),
    UdpSocket(UdpSocket),
}

fn fail(fn_name: &str, msg: impl std::fmt::Display) -> i64 {
    super::throw_str(format!("net.{fn_name}: {msg}"))
}

fn stream_mut<R>(
    fn_name: &str,
    handle: i64,
    f: impl FnOnce(&mut TcpStream) -> Result<R, String>,
) -> Result<R, String> {
    match registry::with_opaque_io::<NetHandle, Option<Result<R, String>>>(
        handle,
        |net| match net {
            NetHandle::TcpStream(mut stream) => {
                let result = f(&mut stream);
                (NetHandle::TcpStream(stream), Some(result))
            }
            other => (other, None),
        },
    ) {
        Some(Some(result)) => result,
        Some(None) => Err(format!("net.{fn_name}: handle is not a TCP stream")),
        None => Err(format!("net.{fn_name}: invalid (null) socket handle")),
    }
}

fn udp_mut<R>(
    fn_name: &str,
    handle: i64,
    f: impl FnOnce(&mut UdpSocket) -> Result<R, String>,
) -> Result<R, String> {
    match registry::with_opaque_io::<NetHandle, Option<Result<R, String>>>(
        handle,
        |net| match net {
            NetHandle::UdpSocket(mut socket) => {
                let result = f(&mut socket);
                (NetHandle::UdpSocket(socket), Some(result))
            }
            other => (other, None),
        },
    ) {
        Some(Some(result)) => result,
        Some(None) => Err(format!("net.{fn_name}: handle is not a UDP socket")),
        None => Err(format!("net.{fn_name}: invalid (null) socket handle")),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_net_tcp_connect(host: i64, port: i64) -> i64 {
    let host = match registry::get_string(host) {
        Some(host) => host,
        None => return fail("tcp_connect", "null string argument"),
    };
    let addr = format!("{host}:{port}");
    match TcpStream::connect(&addr) {
        Ok(stream) => registry::put_opaque(NetHandle::TcpStream(stream)),
        Err(e) => fail("tcp_connect", format!("cannot connect to '{addr}': {e}")),
    }
}

/// `net.tcp_listen(port)` — binds on 0.0.0.0; port 0 picks an ephemeral port
/// (see `net.local_port`).
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_net_tcp_listen(port: i64) -> i64 {
    let addr = SocketAddr::from(([0, 0, 0, 0], port.clamp(0, 65_535) as u16));
    match TcpListener::bind(addr) {
        Ok(listener) => registry::put_opaque(NetHandle::TcpListener(listener)),
        Err(e) => fail("tcp_listen", format!("cannot bind port {port}: {e}")),
    }
}

/// `net.local_port(handle)` — the local port, or -1 when unavailable;
/// useful with port 0.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_net_local_port(handle: i64) -> i64 {
    let outcome = registry::with_opaque(handle, |net: &NetHandle| -> Option<i64> {
        match net {
            NetHandle::TcpListener(listener) => match listener.local_addr() {
                Ok(addr) => Some(addr.port() as i64),
                Err(_) => Some(-1),
            },
            NetHandle::UdpSocket(socket) => match socket.local_addr() {
                Ok(addr) => Some(addr.port() as i64),
                Err(_) => Some(-1),
            },
            _ => None,
        }
    });
    match outcome {
        Some(Some(port)) => port,
        Some(None) => fail("local_port", "handle is not a TCP listener or UDP socket"),
        None => fail("local_port", "invalid (null) socket handle"),
    }
}

/// `net.tcp_accept(listener)` — blocks until a client connects; returns a
/// stream handle.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_net_tcp_accept(handle: i64) -> i64 {
    let outcome =
        registry::with_opaque_io::<NetHandle, Option<Result<TcpStream, String>>>(handle, |net| {
            match net {
                NetHandle::TcpListener(listener) => match listener.accept() {
                    Ok((stream, _peer)) => (NetHandle::TcpListener(listener), Some(Ok(stream))),
                    Err(e) => (
                        NetHandle::TcpListener(listener),
                        Some(Err(format!("cannot accept connection: {e}"))),
                    ),
                },
                other => (other, None),
            }
        });
    match outcome {
        Some(Some(Ok(stream))) => registry::put_opaque(NetHandle::TcpStream(stream)),
        Some(Some(Err(msg))) => fail("tcp_accept", msg),
        Some(None) => fail("tcp_accept", "handle is not a TCP listener"),
        None => fail("tcp_accept", "invalid (null) socket handle"),
    }
}

/// `net.send(handle, data)` — returns the number of bytes written.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_net_send(handle: i64, data: i64) -> i64 {
    let data = registry::get_string(data).unwrap_or_default();
    match stream_mut("send", handle, |stream| -> Result<usize, String> {
        match stream.write_all(data.as_bytes()) {
            Ok(_) => Ok(data.len()),
            Err(e) => Err(format!("net.send: cannot write to stream: {e}")),
        }
    }) {
        Ok(n) => n as i64,
        Err(msg) => {
            let _ = super::throw_str(msg);
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_net_send_line(handle: i64, data: i64) -> i64 {
    let data = registry::get_string(data).unwrap_or_default();
    let mut bytes = data.as_bytes().to_vec();
    bytes.push(b'\n');
    match stream_mut("send_line", handle, |stream| -> Result<usize, String> {
        match stream.write_all(&bytes) {
            Ok(_) => Ok(bytes.len()),
            Err(e) => Err(format!("net.send_line: cannot write to stream: {e}")),
        }
    }) {
        Ok(n) => n as i64,
        Err(msg) => {
            let _ = super::throw_str(msg);
            0
        }
    }
}

/// `net.recv(handle, count)` — up to `count` bytes; "" at end of stream.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_net_recv(handle: i64, count: i64) -> i64 {
    match stream_mut("recv", handle, |stream| -> Result<String, String> {
        if count <= 0 {
            return Ok(String::new());
        }
        let count = (count as usize).min(MAX_RECV_SIZE);
        let mut buf = vec![0u8; count];
        match stream.read(&mut buf) {
            Ok(n) => Ok(String::from_utf8_lossy(&buf[..n]).to_string()),
            Err(e) => Err(format!("net.recv: cannot read from stream: {e}")),
        }
    }) {
        Ok(text) => registry::put_string(text),
        Err(msg) => {
            let _ = super::throw_str(msg);
            0
        }
    }
}

/// `net.recv_line(handle)` — one line, newline included; "" at end of
/// stream.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_net_recv_line(handle: i64) -> i64 {
    match stream_mut("recv_line", handle, |stream| -> Result<String, String> {
        let mut bytes = Vec::with_capacity(64);
        let mut byte = [0u8; 1];
        loop {
            match stream.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {
                    bytes.push(byte[0]);
                    if byte[0] == b'\n' {
                        break;
                    }
                }
                Err(e) => return Err(format!("net.recv_line: cannot read from stream: {e}")),
            }
        }
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }) {
        Ok(text) => registry::put_string(text),
        Err(msg) => {
            let _ = super::throw_str(msg);
            0
        }
    }
}

/// `net.close(handle)` — releases the handle; it must not be used afterwards.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_net_close(handle: i64) -> i8 {
    if registry::take_opaque::<NetHandle>(handle).is_none() {
        let _ = super::throw_str("net.close: invalid (null) socket handle".to_string());
    }
    1
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_net_udp_bind(port: i64) -> i64 {
    let addr = SocketAddr::from(([0, 0, 0, 0], port.clamp(0, 65_535) as u16));
    match UdpSocket::bind(addr) {
        Ok(socket) => registry::put_opaque(NetHandle::UdpSocket(socket)),
        Err(e) => fail("udp_bind", format!("cannot bind port {port}: {e}")),
    }
}

/// `net.udp_send(socket, host, port, data)` — returns the number of bytes
/// sent.
#[unsafe(no_mangle)]
pub extern "C" fn ntsc_net_udp_send(handle: i64, host: i64, port: i64, data: i64) -> i64 {
    let host = match registry::get_string(host) {
        Some(host) => host,
        None => return fail("udp_send", "null string argument"),
    };
    let data = match registry::get_string(data) {
        Some(data) => data,
        None => return fail("udp_send", "null string argument"),
    };
    let addr = format!("{host}:{port}");
    match udp_mut("udp_send", handle, |socket| -> Result<usize, String> {
        match socket.send_to(data.as_bytes(), &addr) {
            Ok(n) => Ok(n),
            Err(e) => Err(format!("net.udp_send: cannot send to '{addr}': {e}")),
        }
    }) {
        Ok(n) => n as i64,
        Err(msg) => {
            let _ = super::throw_str(msg);
            0
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ntsc_net_udp_recv(handle: i64, count: i64) -> i64 {
    match udp_mut("udp_recv", handle, |socket| -> Result<String, String> {
        if count <= 0 {
            return Ok(String::new());
        }
        let count = (count as usize).min(MAX_RECV_SIZE);
        let mut buf = vec![0u8; count];
        match socket.recv(&mut buf) {
            Ok(n) => Ok(String::from_utf8_lossy(&buf[..n]).to_string()),
            Err(e) => Err(format!("net.udp_recv: cannot receive datagram: {e}")),
        }
    }) {
        Ok(text) => registry::put_string(text),
        Err(msg) => {
            let _ = super::throw_str(msg);
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(s: &str) -> i64 {
        registry::put_string(s.to_string())
    }

    fn read(id: i64) -> String {
        let s = registry::get_string(id).unwrap_or_default();
        let _ = registry::take_string(id);
        s
    }

    #[test]
    fn test_tcp_echo() {
        let listener = ntsc_net_tcp_listen(0);
        assert_ne!(listener, 0);
        let port = ntsc_net_local_port(listener);
        assert!(port > 0);

        let client = ntsc_net_tcp_connect(put("127.0.0.1"), port);
        assert_ne!(client, 0);
        let server = ntsc_net_tcp_accept(listener);
        assert_ne!(server, 0);

        assert_eq!(ntsc_net_send_line(client, put("ping")), 5);
        assert_eq!(read(ntsc_net_recv_line(server)), "ping\n");

        assert_eq!(ntsc_net_send(server, put("pong")), 4);
        assert_eq!(read(ntsc_net_recv(client, 4)), "pong");

        assert_eq!(ntsc_net_close(client), 1);
        assert_eq!(ntsc_net_close(server), 1);
        assert_eq!(ntsc_net_close(listener), 1);
    }

    #[test]
    fn test_udp_roundtrip() {
        let receiver = ntsc_net_udp_bind(0);
        assert_ne!(receiver, 0);
        let port = ntsc_net_local_port(receiver);
        assert!(port > 0);

        let sender = ntsc_net_udp_bind(0);
        assert_ne!(sender, 0);
        let sent = ntsc_net_udp_send(sender, put("127.0.0.1"), port, put("hello"));
        assert_eq!(sent, 5);
        assert_eq!(read(ntsc_net_udp_recv(receiver, 64)), "hello");

        assert_eq!(ntsc_net_close(sender), 1);
        assert_eq!(ntsc_net_close(receiver), 1);
    }

    #[test]
    fn test_connect_failure_throws() {
        use crate::modules::test_util::catch_throw;

        let err = catch_throw(|| {
            let host = put("127.0.0.1");
            let _ = ntsc_net_tcp_connect(host, 1);
            let _ = registry::take_string(host);
        });
        let msg = err.unwrap();
        assert!(msg.contains("net.tcp_connect"), "unexpected message: {msg}");
    }

    #[test]
    fn test_wrong_variant_throws() {
        use crate::modules::test_util::catch_throw;
        let listener = ntsc_net_tcp_listen(0);
        let err = catch_throw(|| {
            let data = put("x");
            let _ = ntsc_net_send(listener, data);
            let _ = registry::take_string(data);
        });
        let msg = err.unwrap();
        assert!(msg.contains("net.send"), "unexpected message: {msg}");
        assert_eq!(ntsc_net_close(listener), 1);
    }
}
