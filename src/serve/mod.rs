mod error;

use crate::ServeArgs;
use actix_files::Files;
use actix_web::http::StatusCode;
use actix_web::middleware::Logger;
use actix_web::web::{Data, Path, ServiceConfig};
use actix_web::{get, App, HttpResponse, HttpServer};
use anyhow::{anyhow, bail, Context};
use error::{HttpError, IntoHttpError};
use futures::StreamExt;
use rust_embed::RustEmbed;
use rustls::{Certificate, PrivateKey, ServerConfig};
use std::time::Duration;
use tokio::fs;
use tokio_stream::wrappers::ReadDirStream;

// Generated with:
// openssl req -x509 -newkey rsa:4096 -sha256 -days 14600 -nodes   -keyout "key.pem" \
//  -out "cert.der" -subj "/CN=127.0.0.1" -outform der
// openssl rsa -inform pem -in key.pem -outform der -out key.der
const CERT: &[u8] = include_bytes!("cert.der");
const PKEY: &[u8] = include_bytes!("key.der");

#[derive(RustEmbed)]
#[folder = "viewer/"]
struct Viewer;

#[get("/list")]
async fn list(args: Data<ServeArgs>) -> Result<HttpResponse, HttpError> {
    let dir = ReadDirStream::new(
        fs::read_dir(&args.dir)
            .await
            .context("Unable to read directory")
            .map_500()?,
    );
    let filtered = dir
        .filter_map(|d| async move {
            if let Ok(d) = d {
                if d.path().join("tweets.json").exists() {
                    return Some(d.file_name().to_string_lossy().into_owned());
                };
            }
            None
        })
        .collect::<Vec<_>>()
        .await;
    Ok(HttpResponse::build(StatusCode::OK).json(filtered))
}

#[get("/{path:.*}")]
async fn viewer(path: Path<String>) -> Result<HttpResponse, HttpError> {
    let path = path.into_inner();
    let path = match path.as_str() {
        "" => "index.html",
        any => any,
    };
    let mime = path.rfind('.').map(|idx| {
        let ext = &path[idx + 1..];
        actix_files::file_extension_to_mime(ext)
    });
    match Viewer::get(path) {
        Some(file) => Ok(HttpResponse::build(StatusCode::OK)
            .content_type(mime.unwrap_or(mime::APPLICATION_OCTET_STREAM))
            .body(file.data.into_owned())),
        None => Err(anyhow!("Not found")).map_http_error(StatusCode::NOT_FOUND),
    }
}

fn configure(cfg: &mut ServiceConfig, args: &ServeArgs) {
    cfg.service(list);
    cfg.service(
        Files::new("/dir", &args.dir)
            .prefer_utf8(true)
            .disable_content_disposition(),
    );
    cfg.service(viewer);
}

pub async fn serve(args: ServeArgs) -> anyhow::Result<()> {
    if !args.dir.is_dir() {
        bail!("expected a directory")
    }
    let args2 = args.clone();

    // Using TLS allows us to use ALPN for HTTP/2 which will make serving large
    // quantities of media much quicker
    let tls = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(vec![Certificate(CERT.into())], PrivateKey(PKEY.into()))
        .unwrap();

    let mut server = HttpServer::new(move || {
        App::new()
            .app_data(Data::new(args2.clone()))
            .configure(|s| configure(s, &args2))
            .wrap(Logger::default())
    });
    if !args.no_tls {
        server = server.bind_rustls(args.socket, tls)?;
    } else {
        server = server.bind(args.socket)?;
    }
    let server = server.run();

    if !args.no_launch {
        open_browser(args);
    }

    server.await.context("Unable to run HTTP server")?;
    Ok(())
}

fn open_browser(args: ServeArgs) {
    tokio::task::spawn(async move {
        let url = if args.no_tls {
            format!("http://{}/", args.socket)
        } else {
            format!("https://{}/", args.socket)
        };
        tokio::time::sleep(Duration::from_millis(300)).await;
        open::that(&url).ok();
        log::info!("Hosting at: {}", url);
    });
}
