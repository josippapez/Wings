fn main() {
    if std::env::args_os().len() != 1 {
        eprintln!("Plugin worker accepts no arguments");
        std::process::exit(1);
    }
    if let Err(error) = wings_native::plugin::worker::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
