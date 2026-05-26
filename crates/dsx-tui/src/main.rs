fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed = args.iter().position(|a| a == "--session" || a == "-s")
        .and_then(|i| args.get(i + 1))
        .cloned();

    if let Err(e) = dsx_tui::run(seed) {
        eprintln!("dsx-tui error: {e}");
        std::process::exit(1);
    }
}
