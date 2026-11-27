// SPDX-License-Identifier: GPL-3.0-or-later

fn main() {
    let args = std::env::args_os().skip(1);
    std::process::exit(ice_cli::run(args));
}
