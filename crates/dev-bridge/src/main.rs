use std::path::PathBuf;
use std::sync::Arc;

fn main() {
    let mut data = PathBuf::from("./dev-data");
    let mut port = 8787u16;
    let mut static_dir: Option<PathBuf> = None;
    let mut host = "127.0.0.1".to_string();
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--data" => data = it.next().expect("--data <dir>").into(),
            "--port" => port = it.next().expect("--port <n>").parse().expect("port number"),
            "--host" => host = it.next().expect("--host <addr>"),
            "--static" => static_dir = Some(it.next().expect("--static <dir>").into()),
            "-h" | "--help" => {
                println!("dev-bridge [--data DIR] [--port N] [--host ADDR] [--static ui/dist]");
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    let lib = Arc::new(timeline_core::Library::open(&data).expect("open library"));
    println!("data dir: {}", data.display());
    let res = dev_bridge::serve(lib, dev_bridge::Config { static_dir }, &format!("{host}:{port}"), |a| println!("dev-bridge listening on http://{a}"));
    if let Err(e) = res {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
