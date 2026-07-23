use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::time::Duration;

use webpki_roots;

pub struct HttpResponse {
    pub status:  u16,
    pub headers: Vec<(String, String)>,
    pub body:    String,
}

struct ParsedUrl {
    scheme: String,
    host:   String,
    port:   u16,
    path:   String,
}

fn parse_url(url: &str) -> Result<ParsedUrl, String> {
    let (scheme, rest) = if let Some(r) = url.strip_prefix("http://") {
        ("http", r)
    } else if let Some(r) = url.strip_prefix("https://") {
        ("https", r)
    } else {
        return Err(format!("unsupported or missing URL scheme in '{url}' (only http:// and https:// are recognized)"));
    };

    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None    => (rest, "/"),
    };

    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().map_err(|_| format!("invalid port in URL '{url}'"))?),
        None => (authority.to_string(), if scheme == "https" { 443 } else { 80 }),
    };

    Ok(ParsedUrl { scheme: scheme.to_string(), host, port, path: path.to_string() })
}

fn send_and_read<S: Read + Write>(mut stream: S, method: &str, parsed: &ParsedUrl, body: Option<&str>, headers: &[(String, String)]) -> Result<HttpResponse, String> {
    let body_bytes = body.unwrap_or("");
    let mut req = format!(
        "{} {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: pitruck/1.6\r\n",
        method.to_uppercase(), parsed.path, parsed.host
    );

    let mut has_content_type = false;
    for (k, v) in headers {
        if k.to_lowercase() == "content-type" { has_content_type = true; }
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    if !body_bytes.is_empty() && !has_content_type {
        req.push_str("Content-Type: application/x-www-form-urlencoded\r\n");
    }
    if !body_bytes.is_empty() {
        req.push_str(&format!("Content-Length: {}\r\n", body_bytes.len()));
    }
    req.push_str("\r\n");
    req.push_str(body_bytes);

    stream.write_all(req.as_bytes()).map_err(|e| format!("failed writing request: {e}"))?;
    stream.flush().map_err(|e| format!("failed flushing request: {e}"))?;

    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).map_err(|e| format!("failed reading response: {e}"))?;

    let header_end = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("malformed HTTP response (no header terminator)".to_string())?;
    let header_str = String::from_utf8_lossy(&raw[..header_end]).into_owned();
    let mut lines = header_str.lines();

    let status_line = lines.next().unwrap_or("");
    let status = status_line.split_whitespace().nth(1).and_then(|s| s.parse::<u16>().ok()).unwrap_or(0);

    let mut resp_headers = Vec::new();
    let mut is_chunked = false;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_string();
            let v = v.trim().to_string();
            if k.to_lowercase() == "transfer-encoding" && v.to_lowercase().contains("chunked") {
                is_chunked = true;
            }
            resp_headers.push((k, v));
        }
    }

    let body_bytes_raw = &raw[header_end + 4..];
    let body_string = if is_chunked {
        decode_chunked(body_bytes_raw)
    } else {
        String::from_utf8_lossy(body_bytes_raw).into_owned()
    };

    Ok(HttpResponse { status, headers: resp_headers, body: body_string })
}

pub fn request(method: &str, url: &str, body: Option<&str>, headers: &[(String, String)]) -> Result<HttpResponse, String> {
    let parsed = parse_url(url)?;

    let addr = format!("{}:{}", parsed.host, parsed.port);
    let mut tcp = TcpStream::connect(&addr).map_err(|e| format!("could not connect to {addr}: {e}"))?;
    tcp.set_read_timeout(Some(Duration::from_secs(30))).ok();
    tcp.set_write_timeout(Some(Duration::from_secs(30))).ok();

    if parsed.scheme == "https" {
        let config = make_tls_client_config();
        let server_name = rustls::pki_types::ServerName::try_from(parsed.host.as_str())
            .map_err(|e| format!("invalid TLS server name '{}': {}", parsed.host, e))?
            .to_owned();
        let mut tls = rustls::ClientConnection::new(Arc::new(config), server_name)
            .map_err(|e| format!("failed to create TLS client: {e}"))?;
        let mut stream = rustls::Stream::new(&mut tls, &mut tcp);
        send_and_read(&mut stream, method, &parsed, body, headers)
    } else {
        send_and_read(&mut tcp, method, &parsed, body, headers)
    }
}

fn make_tls_client_config() -> rustls::ClientConfig {
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_protocol_versions(rustls::DEFAULT_VERSIONS)
        .expect("incompatible TLS protocol versions")
        .with_root_certificates(root_store)
        .with_no_client_auth()
}

fn decode_chunked(data: &[u8]) -> String {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let rest = &data[pos..];
        let line_end = match rest.windows(2).position(|w| w == b"\r\n") {
            Some(i) => i,
            None    => break,
        };
        let size_line = String::from_utf8_lossy(&rest[..line_end]);
        let size = usize::from_str_radix(size_line.trim(), 16).unwrap_or(0);
        pos += line_end + 2;
        if size == 0 { break; }
        if pos + size > data.len() { break; }
        out.extend_from_slice(&data[pos..pos + size]);
        pos += size + 2;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

pub fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => { out.push(b' '); i += 1; }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => { out.push(b); i += 3; }
                    Err(_) => { out.push(bytes[i]); i += 1; }
                }
            }
            b => { out.push(b); i += 1; }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
