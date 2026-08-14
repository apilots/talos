fn main() {
    if let Err(error) = talos_linux_sandbox::run_launcher() {
        eprintln!("talos-linux-sandbox-launcher: {error}");
        std::process::exit(1);
    }
}
