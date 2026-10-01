mod cli;
mod engine;
mod gui;
mod utils;

slint::include_modules!();

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        cli::handle_cli(&args)?;
        return Ok(());
    }

    gui::run_gui().map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
}
