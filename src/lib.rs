use std::time::SystemTime;

use axum::{body::Body, http::StatusCode, response::{IntoResponse, Response}};


#[macro_export]
macro_rules! clone_into_closure {
    ( ($( $x:ident ),*) $y:expr ) => {
        {
            $(let $x = $x.clone();)*
            $y
        }
    };
}

pub enum NearsayError {
    ServerError,
    UserNotFound,
    Unauthorized,
    UsernameTaken,
}
impl IntoResponse for NearsayError {
    fn into_response(self) -> Response {
        let status = match self {
            NearsayError::ServerError => StatusCode::INTERNAL_SERVER_ERROR,
            NearsayError::UserNotFound => StatusCode::NOT_FOUND,
            NearsayError::Unauthorized => StatusCode::UNAUTHORIZED,
            NearsayError::UsernameTaken => StatusCode::CONFLICT,
        };

        Response::builder().status(status).body(Body::empty()).unwrap()
    }
}

pub fn current_time_ms() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis().try_into().expect("current time millis doesnt fit into i64")
}