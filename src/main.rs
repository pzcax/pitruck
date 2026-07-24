mod token;
mod error;
mod ast;
mod lexer;
mod parser;
mod value;
mod interpreter;
mod symbol;
mod compiler;
mod json;
mod httpclient;
mod store;
mod cache;

use std::env;
use std::fs;
use std::io::{self, Write, Read, BufRead};
use std::time::Instant;
use std::sync::Arc;

use lexer::Lexer;
use parser::Parser;
use interpreter::Interpreter;
use cache::ProgramCache;
use store::Store;
use value::Value;
use ahash::AHashMap as HashMap;

fn run_source(source: &str, script_path: Option<&str>, show_perf: bool) -> bool {
    run_source_with_perms(source, script_path, show_perf, true, true, true)
}

fn run_source_with_perms(
    source: &str,
    script_path: Option<&str>,
    show_perf: bool,
    allow_read: bool,
    allow_write: bool,
    allow_net: bool,
) -> bool {
    let total = Instant::now();

    let t0 = Instant::now();
    let mut lex = Lexer::new(source);
    let tokens = match lex.tokenize() {
        Ok(t)  => t,
        Err(e) => { eprintln!("{e}"); return false; }
    };
    let lex_ms = t0.elapsed();

    let t1 = Instant::now();
    let mut par = Parser::new(tokens);
    let mut program = match par.parse_program() {
        Ok(p)  => p,
        Err(e) => { eprintln!("{e}"); return false; }
    };
    compiler::resolve_program(&mut program);
    let parse_ms = t1.elapsed();

    let t2 = Instant::now();
    let mut vm = Interpreter::new();
    if let Some(path) = script_path {
        vm.set_script_path(path);
    }

    vm.set_permissions(allow_read, allow_write, allow_net);
    let ok = match vm.run(&program) {
        Ok(_)  => true,
        Err(e) => { eprintln!("{e}"); false }
    };
    let run_ms = t2.elapsed();

    if show_perf {
        eprintln!(
            "\n  lex {:.3}ms  parse {:.3}ms  run {:.3}ms  total {:.3}ms",
            lex_ms.as_secs_f64() * 1000.0,
            parse_ms.as_secs_f64() * 1000.0,
            run_ms.as_secs_f64() * 1000.0,
            total.elapsed().as_secs_f64() * 1000.0,
        );
    }

    ok
}

fn parse_query_to_values(query: &str) -> HashMap<String, Value> {
    if query.is_empty() {
        return HashMap::new();
    }
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() { continue; }
        let mut kv = pair.splitn(2, '=');
        let k = kv.next().unwrap_or("").trim();
        let v = kv.next().unwrap_or("").trim();
        if k.is_empty() { continue; }
        let k = httpclient::url_decode(k);
        let v = httpclient::url_decode(v);
        map.insert(k, Value::Str(v));
    }
    map
}

fn parse_headers_to_values(headers: &[String]) -> HashMap<String, Value> {
    let mut map = HashMap::new();
    for h in headers {
        if let Some((k, v)) = h.split_once(':') {
            let k = k.trim().to_lowercase();
            let v = v.trim().to_string();
            map.insert(k, Value::Str(v));
        }
    }
    map
}

fn content_type_of(headers: &[String]) -> String {
    for h in headers {
        if let Some((k, v)) = h.split_once(':') {
            if k.trim().to_lowercase() == "content-type" {
                return v.trim().to_lowercase();
            }
        }
    }
    String::new()
}

fn serve_request(
    program: &[crate::ast::Stmt],
    script_path: Option<&str>,
    method: &str,
    path: &str,
    query_str: &str,
    body: &str,
    headers: &[String],
    store: Arc<Store>,
    allow_read: bool,
    allow_write: bool,
    allow_net: bool,
    debug: bool,
) -> (u16, String, Vec<(String, String)>) {
    let query_values  = parse_query_to_values(query_str);
    let headers_values = parse_headers_to_values(headers);
    let ct = content_type_of(headers);
    let form_values = if ct.contains("application/x-www-form-urlencoded") {
        parse_query_to_values(body)
    } else {
        HashMap::new()
    };

    let mut vm = Interpreter::new();
    if let Some(sp) = script_path {
        vm.set_script_path(sp);
    }
    vm.set_permissions(allow_read, allow_write, allow_net);
    vm.set_server_store(store);
    
    vm.inject_request_response(
        method, path, query_str,
        query_values, form_values, body, headers_values,
    );

    if let Err(e) = vm.run(program) {
        if debug { eprintln!("[pitruck] runtime error: {e}"); }
        return (500, format!("<pre>Runtime Error\n{e}</pre>"), vec![]);
    }

    let status  = vm.read_response_status().unwrap_or(200.0) as u16;
    let html    = vm.read_response_body().unwrap_or_default();
    let headers = vm.read_response_headers();

    (status, html, headers)
}

fn repl() {
    let stdin  = io::stdin();
    let stdout = io::stdout();

    println!("Pitruck v1.6.1 - type 'exit' to quit");

    let mut vm = Interpreter::new();
    vm.set_permissions(true, true, true);

    loop {
        print!(">> ");
        stdout.lock().flush().unwrap();

        let mut line = String::new();
        if stdin.lock().read_line(&mut line).is_err() { break; }

        let trimmed = line.trim();
        if trimmed == "exit" { break; }
        if trimmed.is_empty() { continue; }

        let t = Instant::now();

        let mut lex = Lexer::new(trimmed);
        let tokens = match lex.tokenize() {
            Ok(t) => t,
            Err(e) => { eprintln!("{e}"); continue; }
        };

        let mut par = Parser::new(tokens);
        let mut program = match par.parse_program() {
            Ok(p)  => p,
            Err(e) => { eprintln!("{e}"); continue; }
        };
        compiler::resolve_program(&mut program);

        if let Err(e) = vm.run(&program) {
            eprintln!("{e}");
        }

        eprintln!("  {:.3}ms", t.elapsed().as_secs_f64() * 1000.0);
    }
}

fn read_http_request_from<S: Read>(stream: &mut S) -> String {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];

    loop {
        match stream.read(&mut tmp) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                buf.extend_from_slice(&tmp[..n]);

                if let Some(header_end) = find_header_end(&buf) {
                    let header_str = String::from_utf8_lossy(&buf[..header_end]);
                    if let Some(len) = parse_content_length(&header_str) {
                        let total = header_end + 4 + len;
                        if buf.len() >= total { break; }
                    } else {
                        break;
                    }
                }
            }
        }
    }

    String::from_utf8_lossy(&buf).into_owned()
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_content_length(headers: &str) -> Option<usize> {
    for line in headers.lines() {
        if line.to_lowercase().starts_with("content-length:") {
            return line.splitn(2, ':').nth(1)?.trim().parse().ok();
        }
    }
    None
}

struct ServerConfig {
    target:      String,
    is_dir:      bool,
    debug:       bool,
    allow_read:  bool,
    allow_write: bool,
    allow_net:   bool,
    cache:       Arc<ProgramCache>,
    store:       Arc<Store>,
    port:        String,
    tls_acceptor: Option<Arc<rustls::ServerConfig>>,
}

fn handle_connection_generic<S: Read + Write>(mut stream: S, cfg: Arc<ServerConfig>) {
    let raw = read_http_request_from(&mut stream);
    let mut lines = raw.lines();

    let request_line = lines.next().unwrap_or("GET / HTTP/1.1");
    let mut parts  = request_line.split_whitespace();
    let method     = parts.next().unwrap_or("GET").to_string();
    let full_path  = parts.next().unwrap_or("/").to_string();

    let (route, query) = if let Some(pos) = full_path.find('?') {
        (full_path[..pos].to_string(), full_path[pos + 1..].to_string())
    } else {
        (full_path.clone(), String::new())
    };
    let route = httpclient::url_decode(&route);

    let mut headers: Vec<String> = Vec::new();
    for hl in lines.by_ref() {
        if hl.is_empty() || hl == "\r" { break; }
        headers.push(hl.trim_end().to_string());
    }
    let body: String = lines.collect::<Vec<_>>().join("\n").trim_matches('\0').to_string();

    let (source, script_path) = if cfg.is_dir {
        let candidate = format!("{}{}.pr", cfg.target.trim_end_matches('/'), route);
        let fallback  = format!("{}/index.pr", cfg.target.trim_end_matches('/'));
        if std::path::Path::new(&candidate).exists() {
            let src = fs::read_to_string(&candidate).unwrap_or_default();
            (src, candidate)
        } else if std::path::Path::new(&fallback).exists() {
            let src = fs::read_to_string(&fallback).unwrap_or_default();
            (src, fallback)
        } else {
            ("response.status = 404\nresponse.body = \"404 Not Found\"".to_string(), String::new())
        }
    } else {
        match fs::read_to_string(&cfg.target) {
            Ok(s)  => (s, cfg.target.clone()),
            Err(e) => { eprintln!("Cannot read '{}': {}", cfg.target, e); return; }
        }
    };

    let program = match cfg.cache.get_or_parse(&script_path, &source) {
        Ok(p)  => p,
        Err(e) => {
            if cfg.debug { eprintln!("[pitruck] parse/cache error: {e}"); }
            let html = format!("<pre>Parse Error\n{e}</pre>");
            let response = format!(
                "HTTP/1.1 500 Internal Server Error\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                html.len(), html
            );
            stream.write_all(response.as_bytes()).ok();
            stream.flush().ok();
            println!("[500] {} {} (cache miss)", method, route);
            return;
        }
    };

    let sp = if script_path.is_empty() { None } else { Some(script_path.as_str()) };

    let t0 = Instant::now();
    let (status_code, html, extra_headers) =
        serve_request(
            &program, sp, &method, &route, &query, &body, &headers,
            cfg.store.clone(),
            cfg.allow_read, cfg.allow_write, cfg.allow_net,
            cfg.debug,
        );
    let elapsed_ms = t0.elapsed().as_secs_f64() * 1000.0;

    let status_text = match status_code {
        200 => "OK", 201 => "Created", 204 => "No Content",
        301 => "Moved Permanently", 302 => "Found",
        400 => "Bad Request", 401 => "Unauthorized",
        403 => "Forbidden", 404 => "Not Found",
        500 => "Internal Server Error", _ => "OK",
    };

    let content_type = extra_headers.iter()
        .find(|(k, _)| k.to_lowercase() == "content-type")
        .map(|(_, v)| v.as_str())
        .unwrap_or_else(|| {
            if html.trim_start().starts_with("<!DOCTYPE") || html.trim_start().starts_with('<') {
                "text/html; charset=utf-8"
            } else {
                "text/plain; charset=utf-8"
            }
        });

    let mut response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        status_code, status_text, content_type, html.len()
    );
    for (k, v) in &extra_headers {
        if k.to_lowercase() != "content-type" {
            response.push_str(&format!("{}: {}\r\n", k, v));
        }
    }
    response.push_str(&format!("\r\n{}", html));

    if let Err(e) = stream.write_all(response.as_bytes()) {
        if cfg.debug { eprintln!("[write] {e}"); }
    }
    if let Err(e) = stream.flush() {
        if cfg.debug { eprintln!("[flush] {e}"); }
    }

    println!("[{}] {} {} ({:.1}ms)", status_code, method, route, elapsed_ms);
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        repl();
        return;
    }

    let debug     = args.contains(&"--debug".to_string());
    let show_perf = args.contains(&"--speed".to_string());

    match args[1].as_str() {
        "--help" => {
            println!("Pitruck v1.6.1");
            println!("Usage: pitruck [command] [args]");
            println!();
            println!("Commands:");
            println!("  [file.pr]                         Run a source file (read/write/net allowed by default)");
            println!("  [file.pr] --speed                 Run and show execution timing");
            println!("  [file.pr] --deny-write            Run with file writes disabled");
            println!("  [file.pr] --deny-all              Run with read/write/net all disabled");
            println!("  --serve <file.pr> [--port N]      Serve file.pr as an HTTP handler");
            println!("  --serve <dir/>    [--port N]      File-based routing from directory");
            println!("  lib install <path|url>            Install a library");
            println!("  lib list                          List installed libraries");
            println!("  lib delete <name>                 Delete a library");
            println!("  --help                            Show this message");
            println!();
            println!("Flags (script mode - default: everything allowed):");
            println!("  --speed            Show lex/parse/run timings");
            println!("  --debug            (No effect in script mode; verbose serve-mode errors)");
            println!("  --allow-read       (No-op in script mode; allowed for muscle-memory compat)");
            println!("  --allow-write      (No-op in script mode; allowed for muscle-memory compat)");
            println!("  --allow-net        (No-op in script mode; allowed for muscle-memory compat)");
            println!("  --allow-all        (No-op in script mode; allowed for muscle-memory compat)");
            println!("  --deny-read        Forbid sys_readfile in script mode");
            println!("  --deny-write       Forbid sys_writefile in script mode");
            println!("  --deny-net         Forbid http_request in script mode");
            println!("  --deny-all         Forbid read, write, and net in script mode");
            println!();
            println!("Flags (serve mode - default: everything denied, opt in below):");
            println!("  --port N           HTTP/HTTPS port (default 8000)");
            println!("  --https            Enable HTTPS with auto-generated dev cert");
            println!("  --https --tls-cert FILE --tls-key FILE  HTTPS with your own cert");
            println!("  --debug            Verbose server error output");
            println!("  --allow-read   Allow file read access (sys_readfile)");
            println!("  --allow-write  Allow file write access (sys_writefile)");
            println!("  --allow-net    Allow outbound HTTP/HTTPS (http_request)");
            println!("  --allow-all    Allow read, write, and net");
            println!();
            println!("HTTPS:");
            println!("  Outbound HTTPS (http_request) is automatic via rustls + webpki-roots.");
            println!("  --https alone: auto-generates a self-signed dev cert cached in");
            println!("    .pitruck-tls/ — zero config for local development.");
            println!("  --https --tls-cert/--tls-key: use your own PEM files for production.");
            println!("  No --https: plain HTTP — serve behind Cloudflare, Nginx, Caddy, etc.");
        }

        "--serve" => {
            if args.len() < 3 {
                eprintln!("Usage: pitruck --serve <file.pr|dir/> [--port N] [--https [--tls-cert FILE --tls-key FILE]] [--allow-read] [--allow-write] [--allow-net] [--allow-all]");
                std::process::exit(1);
            }

            let target = args[2].clone();
            let port = args.iter()
                .position(|a| a == "--port")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("8000");

            let want_https = args.contains(&"--https".to_string());

            let tls_cert_path = args.iter()
                .position(|a| a == "--tls-cert")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.to_string());
            let tls_key_path = args.iter()
                .position(|a| a == "--tls-key")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.to_string());

            if !want_https && (tls_cert_path.is_some() || tls_key_path.is_some()) {
                eprintln!("Error: --tls-cert/--tls-key require --https");
                std::process::exit(1);
            }

            let tls_acceptor = if want_https {
                if tls_cert_path.is_some() && tls_key_path.is_some() {
                    let cp = tls_cert_path.as_ref().unwrap();
                    let kp = tls_key_path.as_ref().unwrap();
                    build_tls_acceptor_from_files(cp, kp)
                } else {
                    build_tls_acceptor_dev()
                }
            } else {
                None  
            };

            let is_user_cert = tls_cert_path.is_some();

            let allow_all   = args.contains(&"--allow-all".to_string());
            let allow_read  = allow_all || args.contains(&"--allow-read".to_string());
            let allow_write = allow_all || args.contains(&"--allow-write".to_string());
            let allow_net   = allow_all || args.contains(&"--allow-net".to_string());

            let addr = format!("0.0.0.0:{}", port);
            let listener = match std::net::TcpListener::bind(&addr) {
                Ok(l)  => l,
                Err(e) => { eprintln!("Cannot bind to {addr}: {e}"); std::process::exit(1); }
            };

            let is_dir = fs::metadata(&target).map(|m| m.is_dir()).unwrap_or(false);
            let is_https = tls_acceptor.is_some();

            println!("Pitruck Server  -  {}://localhost:{}", if is_https { "https" } else { "http" }, port);
            if is_dir {
                println!("Routing         -  {} (file-based)", target);
            } else {
                println!("Handler         -  {}", target);
            }
            if is_https {
                if is_user_cert {
                    println!("TLS             -  enabled (user-supplied cert, rustls + ring)");
                } else {
                    println!("TLS             -  enabled (dev self-signed, cached in .pitruck-tls/)");
                }
            }
            if debug { println!("Mode            -  debug"); }
            println!("Permissions     -  read:{}  write:{}  net:{}",
                if allow_read  { "yes" } else { "no" },
                if allow_write { "yes" } else { "no" },
                if allow_net   { "yes" } else { "no" });
            println!("Concurrency     -  one thread per connection");

            let cfg = Arc::new(ServerConfig {
                target, is_dir, debug,
                allow_read, allow_write, allow_net,
                cache: Arc::new(ProgramCache::new()),
                store: Arc::new(Store::new()),
                port: port.to_string(),
                tls_acceptor,
            });

            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(s)  => s,
                    Err(e) => { eprintln!("[accept] {e}"); continue; }
                };
                let cfg = cfg.clone();
                std::thread::spawn(move || {
                    if let Some(ref tls_cfg) = cfg.tls_acceptor {
                        let mut conn = match rustls::ServerConnection::new(tls_cfg.clone()) {
                            Ok(c)  => c,
                            Err(e) => { eprintln!("[tls] handshake init failed: {e}"); return; }
                        };
                        
                        while conn.is_handshaking() {
                            if let Err(e) = conn.complete_io(&mut stream) {
                                let msg = format!("{e}");
                                if msg.contains("CertificateUnknown") || msg.contains("BadCertificate") || msg.contains("HandshakeFailure") {
                                    eprintln!("[tls] client rejected our certificate (self-signed certs are not trusted by browsers by default)");
                                    eprintln!("[tls] to trust it: open https://localhost:{} in a browser, accept the security warning, then reload", cfg.port);
                                    eprintln!("[tls] or use: curl -k https://localhost:{}/", cfg.port);
                                } else {
                                    eprintln!("[tls] handshake failed: {e}");
                                }
                                return;
                            }
                        }
                        let mut tls_stream = rustls::Stream::new(&mut conn, &mut stream);
                        handle_connection_generic(&mut tls_stream, cfg);
                    } else {
                        handle_connection_generic(&mut stream, cfg);
                    }
                });
            }
        }

        "lib" => {
            if args.len() < 3 {
                eprintln!("Usage: pitruck lib [install|list|delete]");
                std::process::exit(1);
            }
            match args[2].as_str() {
                "install" => {
                    if args.len() < 4 { eprintln!("Usage: pitruck lib install <url_or_path>"); std::process::exit(1); }
                    let source = &args[3];
                    let lib_name = if source.starts_with("http") {
                        source.split('/').last().unwrap_or("unknown").trim_end_matches(".pr")
                    } else if source.ends_with(".pr") {
                        std::path::Path::new(source).file_stem().unwrap().to_str().unwrap()
                    } else {
                        source.as_str()
                    };

                    fs::create_dir_all("lib").unwrap_or_default();
                    let dest = format!("lib/{}.pr", lib_name);

                    if source.starts_with("http") {
                        println!("Downloading '{}'...", source);
                        let ok = std::process::Command::new("curl")
                            .args(["-sL", source, "-o", &dest])
                            .status()
                            .map(|s| s.success())
                            .unwrap_or(false);
                        if ok { println!("Installed '{}' successfully.", lib_name); }
                        else  { eprintln!("Failed to download '{}'.", lib_name); }
                    } else if fs::metadata(source).is_ok() {
                        if fs::copy(source, &dest).is_ok() {
                            println!("Installed '{}' successfully.", lib_name);
                        } else {
                            eprintln!("Failed to copy '{}'.", lib_name);
                        }
                    } else {
                        eprintln!("Source '{}' not found.", source);
                    }
                }
                "list" => {
                    println!("Installed libraries:");
                    if let Ok(entries) = fs::read_dir("lib") {
                        let mut found = false;
                        for entry in entries.flatten() {
                            if let Ok(name) = entry.file_name().into_string() {
                                if name.ends_with(".pr") {
                                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                                    println!("  - {:<20} ({} bytes)", name.trim_end_matches(".pr"), size);
                                    found = true;
                                }
                            }
                        }
                        if !found { println!("  (none)"); }
                    } else {
                        println!("  (none)");
                    }
                }
                "delete" => {
                    if args.len() < 4 { eprintln!("Usage: pitruck lib delete <name>"); std::process::exit(1); }
                    let path = format!("lib/{}.pr", &args[3]);
                    if fs::remove_file(&path).is_ok() {
                        println!("Deleted '{}' successfully.", &args[3]);
                    } else {
                        eprintln!("Library '{}' not found.", &args[3]);
                    }
                }
                other => { eprintln!("Unknown lib subcommand '{}'", other); std::process::exit(1); }
            }
        }

        path => {
            let source = match fs::read_to_string(path) {
                Ok(s)  => s,
                Err(e) => { eprintln!("Cannot read file '{path}': {e}"); std::process::exit(1); }
            };

            let deny_all   = args.contains(&"--deny-all".to_string());
            let deny_read  = deny_all || args.contains(&"--deny-read".to_string());
            let deny_write = deny_all || args.contains(&"--deny-write".to_string());
            let deny_net   = deny_all || args.contains(&"--deny-net".to_string());

            let allow_read  = !deny_read;
            let allow_write = !deny_write;
            let allow_net   = !deny_net;

            if !run_source_with_perms(&source, Some(path), show_perf, allow_read, allow_write, allow_net) {
                std::process::exit(1);
            }
        }
    }
}

fn load_certs(path: &str) -> Vec<rustls::pki_types::CertificateDer<'static>> {
    let data = std::fs::read(path)
        .unwrap_or_else(|e| { eprintln!("Cannot read cert file '{}': {e}", path); std::process::exit(1); });
    let mut reader = std::io::BufReader::new(&data[..]);
    rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|e| { eprintln!("Error parsing cert file '{}': {e}", path); std::process::exit(1); })
}

fn load_private_key(path: &str) -> rustls::pki_types::PrivateKeyDer<'static> {
    let data = std::fs::read(path)
        .unwrap_or_else(|e| { eprintln!("Cannot read key file '{}': {e}", path); std::process::exit(1); });
    let mut reader = std::io::BufReader::new(&data[..]);

    let keys: Vec<rustls_pemfile::Item> = rustls_pemfile::read_all(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_else(|e| { eprintln!("Error parsing key file '{}': {e}", path); std::process::exit(1); });

    for item in keys {
        match item {
            rustls_pemfile::Item::Pkcs8Key(key) => return rustls::pki_types::PrivateKeyDer::Pkcs8(key),
            rustls_pemfile::Item::Pkcs1Key(key) => return rustls::pki_types::PrivateKeyDer::Pkcs1(key),
            rustls_pemfile::Item::Sec1Key(key)   => return rustls::pki_types::PrivateKeyDer::Sec1(key),
            _ => continue,
        }
    }

    eprintln!("No usable private key found in '{}'", path);
    std::process::exit(1);
}

fn build_tls_acceptor_from_files(cert_path: &str, key_path: &str) -> Option<Arc<rustls::ServerConfig>> {
    let certs = load_certs(cert_path);
    let priv_key = load_private_key(key_path);
    let provider = rustls::crypto::ring::default_provider();
    let server_config = rustls::ServerConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(rustls::DEFAULT_VERSIONS)
        .expect("incompatible TLS protocol versions")
        .with_no_client_auth()
        .with_single_cert(certs, priv_key)
        .expect("TLS config error");
    Some(Arc::new(server_config))
}

fn build_tls_acceptor_dev() -> Option<Arc<rustls::ServerConfig>> {
    let tls_dir = ".pitruck-tls";
    let cert_file = format!("{}/cert.pem", tls_dir);
    let key_file  = format!("{}/key.pem", tls_dir);

    if std::path::Path::new(&cert_file).exists() && std::path::Path::new(&key_file).exists() {
        return build_tls_acceptor_from_files(&cert_file, &key_file);
    }

    println!("  Generating dev self-signed cert (cached in .pitruck-tls/)...");
    fs::create_dir_all(tls_dir).unwrap_or_else(|e| {
        eprintln!("Cannot create '{}': {e}", tls_dir);
        std::process::exit(1);
    });

    let (cert_pem, key_pem) = generate_self_signed_cert_pem();

    fs::write(&cert_file, &cert_pem).unwrap_or_else(|e| {
        eprintln!("Cannot write '{}': {e}", cert_file);
        std::process::exit(1);
    });
    fs::write(&key_file, &key_pem).unwrap_or_else(|e| {
        eprintln!("Cannot write '{}': {e}", key_file);
        std::process::exit(1);
    });

    build_tls_acceptor_from_files(&cert_file, &key_file)
}

fn generate_self_signed_cert_pem() -> (String, String) {
    let mut params = rcgen::CertificateParams::new(vec!["localhost".to_string(), "127.0.0.1".to_string()])
        .expect("failed to create cert params");

    params.distinguished_name = rcgen::DistinguishedName::new();
    params.distinguished_name.push(
        rcgen::DnType::CommonName,
        "Pitruck Dev Server",
    );
    params.distinguished_name.push(
        rcgen::DnType::OrganizationName,
        "Pitruck Local Development",
    );

    params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    params.key_usages = vec![rcgen::KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![
        rcgen::ExtendedKeyUsagePurpose::ServerAuth,
    ];

    let key_pair = rcgen::KeyPair::generate().expect("failed to generate key pair");
    let cert = params.self_signed(&key_pair)
        .expect("failed to self-sign certificate");

    let cert_pem = cert.pem();
    let key_pem  = key_pair.serialize_pem();

    (cert_pem, key_pem)
}
