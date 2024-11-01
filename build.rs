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
        // taken from boost.nbt, this is R_SENSE_HDR / R_SENSE_MAIN
        let hdr_factor = 412.0;
        let (scale, offset) = if self.hdr {
            // we offset the high end configs by one dac step so that we don't
            // pick high end configs for an output level of zero
            (1.0, 0.0)
        } else {
            (1.0 / hdr_factor, 0.0)
        };

        scale * (self.dac as f32 / 4096.0) + offset
    }
}

fn possible_levels() -> Vec<PowerLevel> {
    let levels = [false, true]
        .into_iter()
        .flat_map(|hdr| (0..4096).map(move |dac| PowerLevel { hdr, dac }))
        .filter(|l| l.hdr || l.dac != 0)
        .filter(|l| !l.hdr || l.dac > 10) // clip off the low end of the high gear
        .filter(|l| l.hdr || l.dac < 3000) // clip off the upper end of the low gear
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

        println!("cargo:warning=For {l:0.3}: hdr: {hdr}, dac: {dac}");

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
