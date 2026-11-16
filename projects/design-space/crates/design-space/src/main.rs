fn main() {
    if let Err(error) = design_space::cli::run() {
        eprintln!("ds: {error:#}");
        std::process::exit(1);
    }
}
