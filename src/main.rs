#![windows_subsystem = "windows"]

mod app;
mod config;
mod lcu;
mod openai;
mod opgg;
mod types;
mod win32;

use std::sync::Arc;

fn main() {
    let config = config::load_config();
    let rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create tokio runtime"),
    );

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([365.0, 900.0])
            .with_title("LoL OP.GG 辅助"),
        ..Default::default()
    };

    let rt_clone = rt.clone();
    eframe::run_native(
        "LoL OP.GG 辅助",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, config, rt_clone)))),
    )
    .expect("Failed to start eframe");
}
