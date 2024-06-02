# Firmware for my flashlight drivers that use stm32L0 processors

Mostly based off my [AVR flashlight firmware](https://github.com/simmsb/tyrfing/), also written in Rust.


# Running and flashing

To run with debug logging: `env DEFMT_LOG="debug" cargo run`
To flash a non-debug build: `env DEFMT_LOG="off" cargo run --no-default-features --features default_no_debug --release`
