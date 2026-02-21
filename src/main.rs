#![windows_subsystem = "windows"]

mod api;
mod beat;
mod breathing;
mod browser;
mod gateway;
mod media;
mod reactor;
mod settings;
mod types;
mod view;

use reactor::Reactor;

#[tokio::main]
async fn main() -> eframe::Result<()> {
	env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

	let native_options = eframe::NativeOptions {
		viewport: eframe::egui::ViewportBuilder::default()
			.with_inner_size([1280.0, 720.0])
			.with_drag_and_drop(true),
		..Default::default()
	};

	eframe::run_native(
		"Sodglumate",
		native_options,
		Box::new(|cc| Ok(Box::new(Reactor::new(&cc.egui_ctx)))),
	)
}
