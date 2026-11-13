use std::path::Path;

fn main() {
    let cwd = std::env::current_dir().unwrap_or_else(|_| Path::new(".").to_path_buf());
    match mistology::run_cli(&cwd, std::env::args_os()) {
        Ok(out) => print!("{out}"),
        Err(msg) => {
            eprintln!("{msg}");
            std::process::exit(1);
        }
    }
}
