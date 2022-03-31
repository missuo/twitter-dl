use actix_web::http::StatusCode;
use actix_web::ResponseError;
use thiserror::Error;

pub trait IntoHttpError<T>: Sized {
    fn map_http_error(self, code: StatusCode) -> Result<T, HttpError>;

    fn map_500(self) -> Result<T, HttpError> {
        self.map_http_error(StatusCode::INTERNAL_SERVER_ERROR)
    }
}

impl<T, E: std::fmt::Display> IntoHttpError<T> for Result<T, E> {
    fn map_http_error(self, code: StatusCode) -> Result<T, HttpError> {
        self.map_err(|e| HttpError {
            code,
            message: format!("{:#}", e),
        })
    }
}

#[derive(Error, Debug)]
#[error("{message}")]
pub struct HttpError {
    code: StatusCode,
    message: String,
}

impl ResponseError for HttpError {
    fn status_code(&self) -> StatusCode {
        self.code
    }
}
