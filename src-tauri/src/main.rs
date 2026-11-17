#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod desktop;
mod ledger;
mod market;
mod storage;

fn main() {
    desktop::run();
}

#[cfg(test)]
mod checks;
