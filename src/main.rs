#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(any(windows, test))]
mod document;
#[cfg(windows)]
mod editor;
#[cfg(windows)]
mod ui;

#[cfg(windows)]
fn main() {
    ui::start();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("WindRu предназначен для Windows.");
}
