mod error;

use crate::ServeArgs;
use actix_files::Files;
use actix_web::http::StatusCode;
use actix_web::web::{Data, ServiceConfig};
use actix_web::{get, App, HttpResponse, HttpServer};
use anyhow::Context;
use error::{HttpError, IntoHttpError};
use futures::StreamExt;
use tokio::fs;
use tokio_stream::wrappers::ReadDirStream;

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

fn configure(cfg: &mut ServiceConfig, args: &ServeArgs) {
    cfg.service(list);
    cfg.service(Files::new("/dir", &args.dir).prefer_utf8(true));
}

pub async fn serve(args: ServeArgs) -> anyhow::Result<()> {
    let args2 = args.clone();
    let server = HttpServer::new(move || {
        App::new()
            .app_data(Data::new(args2.clone()))
            .configure(|s| configure(s, &args2))
    })
    .bind(args.socket)?
    .run();
    if !args.no_launch {
        open::that(format!("http://{}/list", args.socket)).ok();
    }
    server.await.context("Unable to run HTTP server")?;
    Ok(())
}
