use std::time::SystemTime;

use axum::{body::Body, response::{IntoResponse, Response}};


#[macro_export]
macro_rules! clone_into_closure {
    ( ($( $x:ident ),*) $y:expr ) => {
        {
            $(let $x = $x.clone();)*
            $y
        }
    };
}

#[macro_export]
macro_rules! clone_into_closure_mut {
    ( ($( $x:ident ),*) $y:expr ) => {
        {
            $(let mut $x = $x.clone();)*
            $y
        }
    };
}

#[derive(Debug)]
pub enum NearsayError {
    ServerError,
    UserNotFound,
    Unauthorized,
    UsernameTaken,
}
impl NearsayError {
    pub fn to_status_code(self) -> u16 {
        match self {
            NearsayError::ServerError => 500,
            NearsayError::UserNotFound => 404,
            NearsayError::Unauthorized => 401,
            NearsayError::UsernameTaken => 409,
        }
    }
}
impl IntoResponse for NearsayError {
    fn into_response(self) -> Response {
        Response::builder().status(self.to_status_code()).body(Body::empty()).unwrap()
    }
}

pub fn current_time_ms() -> u64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis().try_into().expect("current time millis doesnt fit into i64")
}