use axum::http::{header::AUTHORIZATION, HeaderMap};
use hmac::{Hmac, Mac};
use jwt::{SignWithKey, VerifyWithKey};
use nearsay_server::NearsayError;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::db::{gen_id, NearsayDB};


#[derive(Serialize, Deserialize)]
pub struct JWTPayload {
    uid: String
}


pub fn get_auth_key() -> Hmac<Sha256> {
    Hmac::new_from_slice(b"some-secret").unwrap()
}

/// returns (jwt, csrf_token)
pub fn create_jwt(key: &Hmac<Sha256>, uid: String) -> Result<String, ()> {
    
    let payload = JWTPayload { uid };

    match payload.sign_with_key(key) {
        Ok(jwt) => Ok(jwt),
        Err(jwt_err) => {
            eprintln!("error creating jwt: {}", jwt_err);
            Err(())
        },
    }

}


/// returns None if no jwt, Some(uid) if success, Err() otherwise
pub fn authenticate_with_header(key: &Hmac<Sha256>, headers: &HeaderMap) -> Result<Option<String>, ()> {
    match headers.get(AUTHORIZATION) {
        None => Ok(None),
        Some(value) => {

            let Ok(value) = value.to_str() else { return Err(()); };
            
            if !value.starts_with("Bearer ") {return Err(()); }

            match authenticate_jwt(key, &value[7..]) {
                Ok(uid) => Ok(Some(uid)),
                Err(err) => Err(err)
            }
        },
    }
}

/// if successful, returns `uid`
pub fn authenticate_jwt(key: &Hmac<Sha256>, jwt: &str) -> Result<String, ()> {

    let verification_result: Result<JWTPayload, jwt::Error> = jwt.verify_with_key(key);

    match verification_result {
        Ok( JWTPayload { uid } ) => Ok(uid),
        Err(jwt_err) => {
            eprintln!("error authenticating jwt: {}", jwt_err);
            Err(())
        }
    }
}




/// returns Ok(uid) if successful
pub async fn create_user(key: &Hmac<Sha256>, db: &NearsayDB, username: &str, userhash: &str) -> Result<String, NearsayError> {
    let uid = gen_id();

    match create_jwt(key, uid) {
        Err(_) => Err(NearsayError::ServerError),
        Ok(uid) => {
            match db.insert_user(&uid, username, userhash).await {
                Ok(_) => Ok(uid),
                Err(err) => Err(err)
            }
        },
    }
}
