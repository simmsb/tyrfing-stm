use std::env;
use std::fs;
use std::path::Path;

use proc_macro2::TokenStream;
use quote::quote;

#[derive(Debug)]
struct PowerLevel {
    hdr: bool,
    dac: u16,
}

impl PowerLevel {
    // output of this level, 0..1
    fn output(&self) -> f32 {
        let (scale, offset) = if self.hdr {
            (1.0, 1.0 / 4096.0)
        } else {
            (1.0 / 4096.0, 0.0)
            // 1 / 3413.33
        };

        scale * (self.dac as f32 / 4096.0) + offset
    }
}

fn possible_levels() -> Vec<PowerLevel> {
    let levels = [false, true]
        .into_iter()
        .flat_map(|hdr| (0..4096).map(move |dac| PowerLevel { hdr, dac }))
        .filter(|l| l.hdr || l.dac != 0)
        .collect::<Vec<_>>();

    levels
}

fn main() {
    let power_levels = 256usize;

    let possible_levels = possible_levels();

    let selected_levels = (1..=power_levels).map(|l| {
        let l = l as f32 / power_levels as f32;
        let l = l.powi(4);

        let PowerLevel { hdr, dac } = possible_levels
            .iter()
            .min_by(|a, b| f32::total_cmp(&(a.output() - l).abs(), &(b.output() - l).abs()))
            .unwrap();

        quote! {
            PowerLevel {
                hdr: #hdr,
                dac: #dac,
            }
        }
    });

    let mut g = TokenStream::new();
    g.extend(quote! {
        pub struct PowerLevel {
            pub hdr: bool,
            pub dac: u16,
        }

        pub const POWER_LEVELS: [PowerLevel; #power_levels] = [
            #(#selected_levels),*
        ];
    });

    let out_dir = env::var_os("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("power_curve.rs");

    fs::write(&dest_path, g.to_string()).unwrap();

    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    println!("cargo:rustc-link-arg-bins=-Tdefmt.x");
}
