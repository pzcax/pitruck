mod token;
mod error;
mod ast;
mod lexer;
mod parser;
mod value;
mod interpreter;

use std::env;
use std::fs;
use std::io::{self, Write, Read, BufRead};
use std::time::Instant;

use lexer::Lexer;
use parser::Parser;
use interpreter::Interpreter;

fn run_source(source: &str, show_perf: bool) -> bool {
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
    let program = match par.parse_program() {
        Ok(p)  => p,
        Err(e) => { eprintln!("{e}"); return false; }
    };
    let parse_ms = t1.elapsed();

    let t2 = Instant::now();
    let mut vm = Interpreter::new();
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

fn query_to_pitruck_dict(query: &str) -> String {
    if query.is_empty() {
        return "{}".to_string();
    }
    let pairs: Vec<String> = query
        .split('&')
        .filter_map(|pair| {
            let mut kv = pair.splitn(2, '=');
            let k = kv.next()?.trim();
            let v = kv.next().unwrap_or("").trim();
            if k.is_empty() { return None; }
            Some(format!("\"{}\": \"{}\"", escape_str(k), escape_str(v)))
        })
        .collect();
    format!("{{{}}}", pairs.join(", "))
}

fn headers_to_pitruck_dict(headers: &[String]) -> String {
    let pairs: Vec<String> = headers
        .iter()
        .filter_map(|h| {
            let mut kv = h.splitn(2, ':');
            let k = kv.next()?.trim().to_lowercase();
            let v = kv.next().unwrap_or("").trim().to_string();
            Some(format!("\"{}\": \"{}\"", escape_str(&k), escape_str(&v)))
        })
        .collect();
    format!("{{{}}}", pairs.join(", "))
}

fn serve_request(
    source: &str,
    method: &str,
    path: &str,
    query: &str,
    body: &str,
    headers: &[String],
    debug: bool,
) -> (u16, String, Vec<(String, String)>) {
    let query_dict   = query_to_pitruck_dict(query);
    let headers_dict = headers_to_pitruck_dict(headers);

    let preamble = format!(
        r#"
class __Request {{
    func init(method, path, query_str, query, body, headers) {{
        self.method    = method
        self.path      = path
        self.query_str = query_str
        self.query     = query
        self.body      = body
        self.headers   = headers
    }}
}}

class __Response {{
    func init() {{
        self.status  = 200
        self.body    = ""
    }}
}}

var request  = __Request({method_lit}, {path_lit}, {query_str_lit}, {query_dict}, {body_lit}, {headers_dict})
var response = __Response()
"#,
        method_lit    = escape_pitruck_str(method),
        path_lit      = escape_pitruck_str(path),
        query_str_lit = escape_pitruck_str(query),
        query_dict    = query_dict,
        body_lit      = escape_pitruck_str(body),
        headers_dict  = headers_dict,
    );

    let full_source = format!("{}\n{}", preamble, source);

    let mut lex = Lexer::new(&full_source);
    let tokens = match lex.tokenize() {
        Ok(t)  => t,
        Err(e) => {
            if debug { eprintln!("[pitruck] lex error: {e}"); }
            return (500, format!("<pre>Lex Error\n{e}</pre>"), vec![]);
        }
    };

    let mut par = Parser::new(tokens);
    let program = match par.parse_program() {
        Ok(p)  => p,
        Err(e) => {
            if debug { eprintln!("[pitruck] parse error: {e}"); }
            return (500, format!("<pre>Parse Error\n{e}</pre>"), vec![]);
        }
    };

    let mut vm = Interpreter::new();
    if let Err(e) = vm.run(&program) {
        if debug { eprintln!("[pitruck] runtime error: {e}"); }
        return (500, format!("<pre>Runtime Error\n{e}</pre>"), vec![]);
    }

    let status  = vm.read_number("response_status")
        .unwrap_or_else(|| vm.read_response_status().unwrap_or(200.0)) as u16;
    let html    = vm.read_response_body()
        .or_else(|| vm.read_string("response_body"))
        .unwrap_or_default();
    let headers = vm.read_response_headers();

    (status, html, headers)
}

fn escape_str(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_pitruck_str(s: &str) -> String {
    format!("\"{}\"", escape_str(s))
}

fn repl() {
    let stdin  = io::stdin();
    let stdout = io::stdout();

    println!("Pitruck v1.3 - type 'exit' to quit");

    let mut vm = Interpreter::new();

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
        let program = match par.parse_program() {
            Ok(p)  => p,
            Err(e) => { eprintln!("{e}"); continue; }
        };

        if let Err(e) = vm.run(&program) {
            eprintln!("{e}");
        }

        eprintln!("  {:.3}ms", t.elapsed().as_secs_f64() * 1000.0);
    }
}

fn read_http_request(stream: &mut std::net::TcpStream) -> String {
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
            println!("Pitruck v1.3");
            println!("Usage: pitruck [command] [args]");
            println!();
            println!("Commands:");
            println!("  [file.pr]                         Run a source file");
            println!("  [file.pr] --speed                 Run and show execution timing");
            println!("  --serve <file.pr> [--port N]      Serve file.pr as an HTTP handler");
            println!("  --serve <dir/>    [--port N]      File-based routing from directory");
            println!("  lib install <path|url>            Install a library");
            println!("  lib list                          List installed libraries");
            println!("  lib delete <name>                 Delete a library");
            println!("  --help                            Show this message");
            println!();
            println!("Flags:");
            println!("  --port N     HTTP port (default 8000)");
            println!("  --debug      Verbose server error output");
            println!("  --speed      Show lex/parse/run timings");
        }

        "--serve" => {
            if args.len() < 3 {
                eprintln!("Usage: pitruck --serve <file.pr|dir/> [--port N]");
                std::process::exit(1);
            }

            let target = &args[2];
            let port = args.iter()
                .position(|a| a == "--port")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("8000");

            let addr = format!("0.0.0.0:{}", port);
            let listener = match std::net::TcpListener::bind(&addr) {
                Ok(l)  => l,
                Err(e) => { eprintln!("Cannot bind to {addr}: {e}"); std::process::exit(1); }
            };

            let is_dir = fs::metadata(target).map(|m| m.is_dir()).unwrap_or(false);

            println!("Pitruck Server  -  http://localhost:{}", port);
            if is_dir {
                println!("Routing         -  {} (file-based)", target);
            } else {
                println!("Handler         -  {}", target);
            }
            if debug { println!("Mode            -  debug"); }

            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(s)  => s,
                    Err(e) => { eprintln!("[accept] {e}"); continue; }
                };

                let raw = read_http_request(&mut stream);
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

                let mut headers: Vec<String> = Vec::new();
                for hl in lines.by_ref() {
                    if hl.is_empty() || hl == "\r" { break; }
                    headers.push(hl.trim_end().to_string());
                }
                let body: String = lines.collect::<Vec<_>>().join("\n").trim_matches('\0').to_string();

                let source = if is_dir {
                    let candidate = format!("{}{}.pr", target.trim_end_matches('/'), route);
                    let fallback  = format!("{}/index.pr", target.trim_end_matches('/'));
                    fs::read_to_string(&candidate)
                        .or_else(|_| fs::read_to_string(&fallback))
                        .unwrap_or_else(|_| "response.status = 404\nresponse.body = \"404 Not Found\"".to_string())
                } else {
                    match fs::read_to_string(target) {
                        Ok(s)  => s,
                        Err(e) => { eprintln!("Cannot read '{}': {}", target, e); continue; }
                    }
                };

                let t0 = Instant::now();
                let (status_code, html, extra_headers) =
                    serve_request(&source, &method, &route, &query, &body, &headers, debug);
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
                    eprintln!("[write] {e}");
                }
                if let Err(e) = stream.flush() {
                    eprintln!("[flush] {e}");
                }

                println!("[{}] {} {} ({:.1}ms)", status_code, method, route, elapsed_ms);
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
            if !run_source(&source, show_perf) {
                std::process::exit(1);
            }
        }
    }
}