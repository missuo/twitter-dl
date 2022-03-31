mod error;

use crate::ServeArgs;
use actix_files::Files;
use actix_web::http::StatusCode;
use actix_web::web::{Data, Path, ServiceConfig};
use actix_web::{get, App, HttpResponse, HttpServer};
use anyhow::{anyhow, bail, Context};
use error::{HttpError, IntoHttpError};
use futures::StreamExt;
use rust_embed::RustEmbed;
use tokio::fs;
use tokio_stream::wrappers::ReadDirStream;

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
    cfg.service(Files::new("/dir", &args.dir).prefer_utf8(true));
    cfg.service(viewer);
}

pub async fn serve(args: ServeArgs) -> anyhow::Result<()> {
    if !args.dir.is_dir() {
        bail!("expected a directory")
    }
    let args2 = args.clone();
    let server = HttpServer::new(move || {
        App::new()
            .app_data(Data::new(args2.clone()))
            .configure(|s| configure(s, &args2))
    })
    .bind(args.socket)?
    .run();
    if !args.no_launch {
        open::that(format!("http://{}/", args.socket)).ok();
    }
    server.await.context("Unable to run HTTP server")?;
    Ok(())
}
