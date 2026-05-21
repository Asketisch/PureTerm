fn main() {
    if let Err(err) = pureterm_alacritty_runtime::run_from_env() {
        eprintln!("PureTerm: {err}");
        std::process::exit(1);
    }
}
