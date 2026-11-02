//! Start the local server for the web app. See the library for what it serves.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use crucible_server::app;

const USAGE: &str = "\
usage: crucible-server [options]

  --port N      port to listen on (default 8000)
  --root DIR    the folder holding scenarios/ and web/build (default: here)
";

fn options() -> Result<(u16, Option<PathBuf>), String> {
    let (mut port, mut root) = (8000u16, None);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("`{arg}` needs a value"));
        match arg.as_str() {
            "-h" | "--help" => return Err(String::new()),
            "--port" => {
                port = value()?
                    .parse()
                    .map_err(|_| "`--port` needs a number".to_string())?;
            }
            "--root" => root = Some(PathBuf::from(value()?)),
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok((port, root))
}

#[tokio::main]
async fn main() -> ExitCode {
    let (port, root) = match options() {
        Ok(o) => o,
        Err(message) => {
            if !message.is_empty() {
                eprintln!("crucible-server: {message}\n");
            }
            eprint!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    // A scenario names its sources relative to the root, so that is where
    // the server has to be standing.
    if let Some(root) = &root {
        if let Err(e) = std::env::set_current_dir(root) {
            eprintln!("crucible-server: cannot use {}: {e}", root.display());
            return ExitCode::FAILURE;
        }
    }
    let root = match std::env::current_dir() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("crucible-server: {e}");
            return ExitCode::FAILURE;
        }
    };
    if !root.join("web").join("build").join("index.html").is_file() {
        eprintln!(
            "crucible-server: no built front end in web/build - run `npm install` and \
             `npm run build` in web/ (the API still answers)"
        );
    }

    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = match tokio::net::TcpListener::bind(address).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("crucible-server: cannot listen on {address}: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("crucible: http://{address}  (serving {})", root.display());
    match axum::serve(listener, app(root)).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("crucible-server: {e}");
            ExitCode::FAILURE
        }
    }
}
