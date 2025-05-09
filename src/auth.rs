
use axum::http::{header::AUTHORIZATION, HeaderMap};
use hmac::Hmac;
use jwt::{SignWithKey, VerifyWithKey};
use serde::{Deserialize, Serialize};
use sha2::Sha256;



#[derive(Serialize, Deserialize)]
pub struct JWTPayload {
    pub uid: String,
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


pub fn verify_password(password: &str, hash: &str) -> Result<bool, ()> {
    match bcrypt::verify(password, hash) {
        Err(bcrypt_err) => {
            eprintln!("bcrypt error when authorizing user: {}", bcrypt_err);
            Err(())
        },
        Ok(verified) => Ok(verified)
    }
}


/// returns None if no jwt, OK(Some(JWTPayload)) if success, Ok(None) if no header, Err() otherwise
pub fn authenticate_with_header(key: &Hmac<Sha256>, headers: &HeaderMap) -> Result<Option<JWTPayload>, ()> {
    match headers.get(AUTHORIZATION) {
        None => Ok(None),
        Some(value) => {

            let Ok(value) = value.to_str() else { return Err(()); };
            
            if !value.starts_with("Bearer ") {return Err(()); }

            match authenticate_jwt(key, &value[7..]) {
                Ok(payload) => Ok(Some(payload)),
                Err(err) => Err(err)
            }
        },
    }
}

/// if successful, returns `uid`
pub fn authenticate_jwt(key: &Hmac<Sha256>, jwt: &str) -> Result<JWTPayload, ()> {

    let verification_result: Result<JWTPayload, jwt::Error> = jwt.verify_with_key(key);

    match verification_result {
        Err(jwt_err) => {
            eprintln!("error authenticating jwt: {}", jwt_err);
            Err(())
        }
        Ok(payload) => Ok(payload),
    }
}


