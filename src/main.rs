mod api;
mod app;

use app::SodglumateApp;

#[tokio::main]
async fn main() -> eframe::Result<()> {
	env_logger::init();

	let native_options = eframe::NativeOptions {
		viewport: eframe::egui::ViewportBuilder::default()
			.with_inner_size([1280.0, 720.0])
			.with_drag_and_drop(true),
		..Default::default()
	};

	eframe::run_native(
		"Sodglumate",
		native_options,
		Box::new(|cc| Ok(Box::new(SodglumateApp::new(cc)))),
	)
}
