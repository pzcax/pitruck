@echo off

IF NOT EXIST "Cargo.toml" (
    (
        echo [package]
        echo name = "Pitruck"
        echo version = "1.5.0"
        echo edition = "2021"

        echo [profile.release]
        echo opt-level = 3
        echo lto = "fat"
        echo codegen-units = 1
        echo panic = "abort"
        echo strip = true
        echo ahash = "0.8"

    ) > Cargo.toml
)
set RUSTFLAGS=-Awarnings
cargo build --release --quiet