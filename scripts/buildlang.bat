@echo off

IF NOT EXIST "Cargo.toml" (
    echo Making Cargo.toml...

    (
        echo [package]
        echo name = "Pitruck"
        echo version = "1.3.0"
        echo edition = "2021"
        echo.
        echo [dependencies]
    ) > Cargo.toml
)
set RUSTFLAGS=-Awarnings
cargo build --release --quiet