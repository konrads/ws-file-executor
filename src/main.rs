//! Upload File Executor
//!
//! Uses axis_web to create a web server that accepts file uploads and executes scripts.

use std::{env, sync::Arc};

use actix_files::NamedFile;
use actix_multipart::form::{tempfile::TempFile, text::Text, MultipartForm};
use actix_web::{
    error::{self, ErrorInternalServerError},
    middleware, rt, web, App, Error, HttpRequest, HttpResponse, HttpServer, Responder,
};
use actix_ws::{CloseCode, CloseReason};
use serde_derive::Deserialize;
use tokio::sync::mpsc;
use uuid::Uuid;

mod constants;
use constants::*;
mod service;
use service::{ProdServices, Services};
mod defer;

#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod tests;

/// App creation in a macro allows for route testing
#[macro_export]
macro_rules! init_app_routes {
    ($services:expr) => {
        App::new()
            .app_data(web::Data::new($services.clone()))
            // WebSocket UI HTML file
            .service(web::resource("/").to(index))
            // websocket cmd execution route
            .service(web::resource("/runCommand").route(web::get().to(run_cmd_ws)))
            // upload route
            .service(web::resource("/upload").route(web::post().to(upload)))
            // enable logger
            .wrap(middleware::Logger::default())
    };
}

/// Upload multipart form, comprises of the file
#[derive(Debug, MultipartForm)]
struct UploadFileForm {
    #[multipart]
    file_path: Text<String>,
    #[multipart]
    file: TempFile,
}

/// Execute command parameters, comprises file id and command to execute
#[derive(Debug, Deserialize)]
pub struct ExecParams {
    id: String,
    cmd: String,
}

/// Static /index.html route
async fn index() -> Result<impl Responder, Error> {
    Ok(NamedFile::open_async("./static/index.html").await?)
}

/// Upload file route
async fn upload(
    services: web::Data<Arc<dyn Services>>,
    MultipartForm(form): MultipartForm<UploadFileForm>,
) -> Result<HttpResponse, Error> {
    let uuid = Uuid::new_v4();
    let file_path = form.file_path.as_str();
    let file_name = form
        .file
        .file_name
        .ok_or(error::ErrorBadRequest("file not specified"))?;
    let full_path = format!("{file_path}/{file_name}");

    let mut read_file = form.file.file;
    services
        .register_file(&uuid.to_string(), &full_path, read_file.as_file_mut())
        .await
        .map_err(|_| {
            ErrorInternalServerError(format!(
                "Failed to upload file {uuid}/{file_path}/{file_name}"
            ))
        })?;

    log::info!("saved file to {uuid}/{file_path}/{file_name} under id {uuid}");
    Ok(HttpResponse::Ok()
        .append_header((X_FILE_ID_HEADER, uuid.to_string()))
        .finish())
}

/// Execute command via websocket
async fn run_cmd_ws(req: HttpRequest, stream: web::Payload) -> Result<HttpResponse, Error> {
    let (res, mut session, _msg_stream) = actix_ws::handle(&req, stream)?;
    let (sender, mut receiver) = mpsc::channel::<String>(1024);
    let services = req
        .app_data::<web::Data<Arc<dyn Services>>>()
        .ok_or::<Error>(ErrorInternalServerError("Failed to get app_data!!!"))?
        .clone();

    let params = web::Query::<ExecParams>::from_query(req.query_string())?;

    let id = params.id.clone();
    rt::spawn(async move {
        let cmd_task =
            rt::spawn(async move { services.run_cmd(&params.id, &params.cmd, sender).await });

        while let Some(line) = receiver.recv().await {
            log::info!("received stdout line: {line}");
            session
                .text(line)
                .await
                .unwrap_or_else(|_| log::error!("Failed to send stdout"));
        }

        let close_reason = match cmd_task.await {
            Ok(Ok(_)) => None,
            _ => Some(CloseReason::from((
                CloseCode::Error,
                "Command execution failure",
            ))),
        };

        session
            .close(close_reason)
            .await
            .unwrap_or_else(|_| log::error!("Failed to close session"));
        log::info!("Closed session {id}");
        Ok::<(), Error>(())
    });

    Ok(res)
}

/// Main function, starts the web server and registers routes
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let current_dir = env::current_dir()?.display().to_string();
    let uploads_dir = format!("{current_dir}/{UPLOADS_DIR}");
    log::info!(
        "

-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-
    UPLOAD FILE EXECUTOR

    - host:        {HOST}
    - port:        {PORT}
    - uploads_dir: {uploads_dir}
-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-
",
    );

    let services_arc: Arc<dyn Services> = Arc::new(ProdServices::new(uploads_dir));

    HttpServer::new(move || init_app_routes!(services_arc))
        .workers(8)
        .bind((HOST, PORT))?
        .run()
        .await
}
