#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adb;
mod app;
mod operations;
mod store;

fn main() {
    app::run();
}
