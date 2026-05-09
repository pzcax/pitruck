@echo off

IF NOT EXIST "Cargo.toml" (
    echo making Cargo.toml...

    (
        echo [package]
        echo name = "Pitruck"
        echo version = "1.3.0"
        echo edition = "2021"
        echo.
        echo [dependencies]
    ) > Cargo.toml
)

cargo build --release --quiet