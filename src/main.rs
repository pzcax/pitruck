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

    let lex_start_time = Instant::now();
    let mut lex = Lexer::new(source);
    let tokens = match lex.tokenize() {
        Ok(t)  => t,
        Err(e) => { eprintln!("{e}"); return false; }
    };
    let lex_ms = lex_start_time.elapsed();

    let parse_start_time = Instant::now();
    let mut par = Parser::new(tokens);
    let program = match par.parse_program() {
        Ok(p)  => p,
        Err(e) => { eprintln!("{e}"); return false; }
    };
    let parse_ms = parse_start_time.elapsed();

    let run_start_time = Instant::now();
    let mut vm = Interpreter::new();
    let ok = match vm.run(&program) {
        Ok(_)  => true,
        Err(e) => { eprintln!("{e}"); false }
    };
    let run_ms = run_start_time.elapsed();

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

fn repl() {
    let stdin  = io::stdin();
    let stdout = io::stdout();

    println!("Pitruck v0.1 - type 'exit' to quit");

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
            Err(e) => {
                println!("{}", e);
                std::process::exit(1);
            }
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

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        repl();
        return;
    }

    let cmd = args[1].as_str();

    match cmd {
        "--help" => {
            println!("Pitruck Compiler v0.1");
            println!("Usage: pitruck [command] [args]");
            println!("");
            println!("Commands:");
            println!("  [file.pr]                  Run a pitruck source file");
            println!("  [file.pr] --speed          Run and show execution speed telemetry");
            println!("  lib install <path/url>     Install a library locally or from a URL");
            println!("  lib list                   List installed libraries");
            println!("  lib delete <library>       Delete a library");
            println!("  update --all               Update all libraries");
            println!("  update <library>           Update a specific library");
            println!("  upgrade                    Upgrade pitruck to the latest version via cargo");
            println!("  doctor                     Fix pitruck environment issues");
            println!("  serve <port>               Run a local web server for your masterpieces");
            println!("  --help                     Show this help message");
            return;
        }
        "serve" => {
            let port = args.get(2).map(|s| s.as_str()).unwrap_or("8000");
            let addr = format!("127.0.0.1:{}", port);
            let listener = match std::net::TcpListener::bind(&addr) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("Could not bind to {}: {}", addr, e);
                    std::process::exit(1);
                }
            };
            println!("Pitruck Server running at http://{}", addr);
            for stream in listener.incoming() {
                if let Ok(mut stream) = stream {
                    let mut buffer = [0; 1024];
                    if stream.read(&mut buffer).is_ok() {
                        let request = String::from_utf8_lossy(&buffer);
                        let path = request.split_whitespace().nth(1).unwrap_or("/");
                        let file_path = if path == "/" { "index.html" } else { &path[1..] };
                        
                        let (status, content, content_type) = match fs::read(file_path) {
                            Ok(c) => {
                                let ctype = if file_path.ends_with(".css") { "text/css" } else { "text/html; charset=utf-8" };
                                ("HTTP/1.1 200 OK", c, ctype)
                            }
                            Err(_) => ("HTTP/1.1 404 NOT FOUND", b"404 - Masterpiece Not Found".to_vec(), "text/plain"),
                        };
                        
                        let response = format!("{}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", status, content_type, content.len());
                        stream.write_all(response.as_bytes()).ok();
                        stream.write_all(&content).ok();
                        stream.flush().ok();

                    }
                }
            }
            return;
        }
        "lib" => {
            if args.len() < 3 {
                eprintln!("Usage: pitruck lib [install|list|delete]");
                std::process::exit(1);
            }
            let subcmd = args[2].as_str();
            match subcmd {
                "install" => {
                    if args.len() < 4 {
                        eprintln!("Usage: pitruck lib install <url_or_path>");
                        std::process::exit(1);
                    }
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
                        let status = std::process::Command::new("curl")
                            .args(["-sL", source, "-o", &dest])
                            .status();
                        match status {
                            Ok(s) if s.success() => println!("Installed '{}' successfully.", lib_name),
                            _ => println!("Failed to download library '{}'.", lib_name),
                        }
                    } else if fs::metadata(source).is_ok() {
                        if fs::copy(source, &dest).is_ok() {
                            println!("Installed '{}' successfully.", lib_name);
                        } else {
                            println!("Failed to copy library '{}'.", lib_name);
                        }
                    } else {
                        println!("Could not resolve source '{}'. If this is a local path, the file does not exist. If this is a package name, the registry is offline.", source);
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
                                    println!("  - {:<15} ({} bytes)", name.trim_end_matches(".pr"), size);
                                    found = true;
                                }
                            }
                        }
                        if !found {
                            println!("  (none)");
                        }
                    } else {
                        println!("  (none)");
                    }
                }
                "delete" => {
                    if args.len() < 4 {
                        eprintln!("Usage: pitruck lib delete <library>");
                        std::process::exit(1);
                    }
                    let lib_name = &args[3];
                    let path = format!("lib/{}.pr", lib_name);
                    if fs::remove_file(&path).is_ok() {
                        println!("Deleted library '{}' successfully.", lib_name);
                    } else {
                        println!("Library '{}' not found.", lib_name);
                    }
                }
                _ => {
                    eprintln!("Unknown lib command '{}'", subcmd);
                    std::process::exit(1);
                }
            }
            return;
        }
        "update" => {
            if args.len() < 3 {
                eprintln!("Usage: pitruck update <library> | pitruck update --all");
                std::process::exit(1);
            }
            if args[2] == "--all" {
                println!("Updating all libraries... (Requires configured package registry)");
            } else {
                println!("Updating library '{}'... (Requires configured package registry)", args[2]);
            }
            return;
        }
        "upgrade" => {
            println!("Upgrading Pitruck...");
            if std::path::Path::new("Cargo.toml").exists() {
                println!("Found Cargo workspace. Running 'cargo build --release'...");
                let status = std::process::Command::new("cargo")
                    .args(["build", "--release"])
                    .status();
                if status.map_or(false, |s| s.success()) {
                    println!("Upgrade complete! New binary available in target/release/");
                } else {
                    println!("Failed to upgrade via cargo.");
                }
            } else {
                println!("No Cargo.toml found. Please download the latest binary manually.");
            }
            return;
        }
        "doctor" => {
            println!("Running Pitruck Doctor...");
            let mut issues = 0;
            if !std::path::Path::new("lib").exists() {
                println!("[-] 'lib' directory is missing. Creating...");
                fs::create_dir_all("lib").unwrap_or_default();
                issues += 1;
            } else {
                println!("[+] 'lib' directory exists.");
            }
            
            let stdlibs = ["system", "time", "color", "math"];
            for lib in stdlibs {
                let path = format!("lib/{}.pr", lib);
                if !std::path::Path::new(&path).exists() {
                    println!("[-] Standard library '{}' is missing. Recreating stub...", lib);
                    fs::write(&path, format!("Recovered {} library\n", lib)).unwrap_or_default();
                    issues += 1;
                } else {
                    let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    if size == 0 {
                        println!("[-] Standard library '{}' is empty. Recreating stub...", lib);
                        fs::write(&path, format!("Recovered {} library\n", lib)).unwrap_or_default();
                        issues += 1;
                    } else {
                        println!("[+] Standard library '{}' is healthy ({} bytes).", lib, size);
                    }
                }
            }
            
            println!("[+] Rust Cargo available: {}", std::process::Command::new("cargo").arg("--version").status().is_ok());
            println!("[+] Curl available: {}", std::process::Command::new("curl").arg("--version").status().is_ok());
            
            if issues == 0 {
                println!("Doctor summary: Everything is perfectly healthy!");
            } else {
                println!("Doctor summary: Fixed {} issues.", issues);
            }
            return;
        }
        _ => {}
    }

    let path = &args[1];
    let show_perf = args.len() > 2 && args[2] == "--speed";

    let source = match fs::read_to_string(path) {
        Ok(s)  => s,
        Err(e) => {
            eprintln!("Cannot read file '{path}': {e}");
            std::process::exit(1);
        }
    };

    if !run_source(&source, show_perf) {
        std::process::exit(1);
    }
}
