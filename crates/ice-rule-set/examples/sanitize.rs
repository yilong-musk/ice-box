// SPDX-License-Identifier: GPL-3.0-or-later

//! Sanitize a rule-set file: `cargo run -p ice-rule-set --example sanitize -- <in> <out.json>`.

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(input), Some(output)) = (args.next(), args.next()) else {
        eprintln!("usage: sanitize <input .srs|.json> <output .json>");
        std::process::exit(2);
    };
    let bytes = std::fs::read(&input).expect("read input");
    match ice_rule_set::sanitize(&bytes, &ice_rule_set::Limits::default()) {
        Ok(out) => {
            std::fs::write(&output, &out.json).expect("write output");
            println!("{input}: {:?}", out.stats);
        }
        Err(err) => {
            eprintln!("{input}: {err}");
            std::process::exit(1);
        }
    }
}
