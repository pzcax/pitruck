@echo off

IF NOT EXIST "Cargo.toml" (
    (
        echo [package]
        echo name = "pitruck"
        echo version = "1.6.0"
        echo edition = "2021"

        echo [[bin]]
        echo name = "pitruck"
        echo path = "src/main.rs"

        echo [profile.release]
        echo opt-level = 3
        echo lto = "fat"
        echo codegen-units = 1
        echo panic = "abort"
        echo strip = true

        echo [dependencies]
        echo ahash = "0.8.12"
        echo rustls = { version = "0.23", default-features = false, features = ["ring", "std", "tls12"] }
        echo webpki-roots = "0.26"
        echo rustls-pemfile = "2"
        echo rcgen = { version = "0.13", features = ["pem"] }
    ) > Cargo.toml
)
set RUSTFLAGS=-Awarnings
cargo build --release --quiet