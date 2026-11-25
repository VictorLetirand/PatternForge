fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = pf_cli::run(&args, &mut std::io::stdout(), &mut std::io::stderr());
    std::process::exit(i32::from(code));
}
