@echo off

IF NOT EXIST "Cargo.toml" (
    echo makin Cargo.toml...

    (
        echo [package]
        echo name = "Pitruck"
        echo version = "1.2.0"
        echo edition = "2021"
        echo.
        echo [dependencies]
    ) > Cargo.toml
)

cargo build --release