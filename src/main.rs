use TREOVL::cli;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().len() > 1 {
        cli::handle_cli()?;
        return Ok(());
    }

    #[cfg(feature = "gui")]
    {
        TREOVL::gui::run_gui().map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    #[cfg(not(feature = "gui"))]
    {
        eprintln!("TREOVL: Running in headless/CLI-only mode (GUI feature disabled).");
        eprintln!("Run `TREOVL --help` for available commands.");
        std::process::exit(1);
    }
}
