use axum::http::{header::AUTHORIZATION, HeaderMap};
use hmac::{Hmac, Mac};
use jwt::{SignWithKey, VerifyWithKey};
use nearsay_server::NearsayError;
use sha2::Sha256;

use crate::{db::{gen_id, NearsayDB}, types::User};


pub fn get_auth_key() -> Hmac<Sha256> {
    Hmac::new_from_slice(b"some-secret").unwrap()
}

/// returns (jwt, csrf_token)
pub fn create_jwt(key: &Hmac<Sha256>, uid: String) -> Result<String, ()> {
    
    match uid.sign_with_key(key) {
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

    match jwt.verify_with_key(key) {
        Ok(uid) => Ok(uid),
        Err(jwt_err) => {
            eprintln!("error authenticating jwt: {}", jwt_err);
            Err(())
        }
    }
}




/// returns Ok(uid) if successful
pub async fn create_user(key: &Hmac<Sha256>, db: &NearsayDB, username: String, userhash: String) -> Result<String, NearsayError> {
    let uid = gen_id();

    match create_jwt(key, uid.clone()) {
        Err(_) => Err(NearsayError::ServerError),
        Ok(uid) => {
            let user = User {
                _id: uid.clone(),
                username,
                hash: userhash,
            };

            match db.insert_user(user).await {
                Ok(_) => Ok(uid),
                Err(err) => Err(err)
            }
        },
    }
}
