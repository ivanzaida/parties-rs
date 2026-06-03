mod app;
mod network;
mod screens;
mod theme;

fn main() {
  let mut lurq_app = lurq::app::App::new();
  let assets = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
  lurq_app.set_resource_root(assets.clone());
  lurq_app.load_fonts_dir(assets.join("fonts").as_path());
  lurq_app.register_font("Inter", "Inter");
  lurq_app.register_font("JetBrains Mono", "JetBrains Mono");
  lurq_app.register_font("Lucide", "Lucide");
  lurq::app::devtools::load_fonts(&mut lurq_app);
  theme::setup(lurq_app.theme());

  let mut tree = lurq::app::runtime::Tree::new();
  tree.set_render_engine_factory(|| Box::new(lurq::app::wgpu_render::WgpuRenderEngine::new()));
  tree.mount_root::<app::App>(lurq_app.theme().clone(), ());
  tree.mount_devtools(lurq_app.theme().clone());

  lurq::app::winit_shell::WinitWindow::new(lurq_app, tree)
    .with_title("Parties")
    .with_size(1280, 900)
    .on_tick(lurq::app::runtime::Tree::request_redraw)
    .run();
}
