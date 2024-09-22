# Hardware and firmware for my flashlight drivers that use stm32L0 processors

<p float="left" align="middle">
  <img align="top" width="32%" alt="image" src="/hw/Driver.jpg">
  <img align="top" width="32%" alt="image" src="https://github.com/user-attachments/assets/2c77e7bf-9297-43a6-a54f-000c5263372f">
  <img align="top" width="32%" alt="image" src="https://github.com/user-attachments/assets/2a81bbc3-8534-478d-a644-1a32fab9abc6">
</p>

This repo contains the KiCad files of my self-designed flashlight driver (stm32L0, using a MP3432 boost driver) and the Rust firmware for it.

Mostly based off my [AVR flashlight firmware](https://github.com/simmsb/tyrfing/), also written in Rust.


# Running and flashing

To run with debug logging: `env DEFMT_LOG="debug" cargo run`
To flash a non-debug build: `env DEFMT_LOG="off" cargo run --no-default-features --features default_no_debug --release`
