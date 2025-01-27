use axum::{body::Body, http::{HeaderMap, StatusCode}, response::{IntoResponse, Response}};
use axum_extra::extract::CookieJar;
use hmac::{Hmac, Mac};
use jwt::{SignWithKey, VerifyWithKey, Error as JwtError};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;

use crate::{db::{gen_id, NearsayDB}, types::User};

#[derive(Serialize, Deserialize, Debug)]
struct JWTBody {
    uid: String,
    csrf_token: String, 
}

pub struct AuthPair {
    pub jwt: String,
    pub csrf_token: String,
}
impl IntoResponse for AuthPair {
    fn into_response(self) -> Response {
        Response::builder()
            .status(200)
            .header("SET-COOKIE", format!("jwt={}", self.jwt))
            .header("CSRF-TOKEN", self.csrf_token)
            .body(Body::empty())
            .unwrap()
    }
}

/// returns (jwt, csrf_token)
pub fn create_jwt_and_csrf(uid: String) -> Result<AuthPair, JwtError> {
    
    let key: Hmac<Sha256> = Hmac::new_from_slice(b"some-secret").unwrap();

    let csrf_token = Uuid::new_v4().to_string();

    let claims = JWTBody { uid, csrf_token: csrf_token.clone() };

    match claims.sign_with_key(&key) {
        Ok(jwt) => Ok(AuthPair {jwt, csrf_token}),
        Err(err) => {
            eprintln!("error creating jwt: {}", &err);
            Err(err)
        },
    }

}

/// if successful, returns `uid`
pub fn authenticate(headers: &HeaderMap, cookies: &CookieJar) -> Result<String, ()> {
    
    let Some(jwt) = cookies.get("jwt") 
    else { return Err(()); };
    let jwt = jwt.value_trimmed();
    
    let Some(expected_csrf_token) = headers.get("csrf-token")
    else { return Err(()); };
    let expected_csrf_token = expected_csrf_token.to_str().unwrap();
    
    let key: Hmac<Sha256> = Hmac::new_from_slice(b"some-secret").unwrap();

    match jwt.verify_with_key(&key) {
        Ok(claim) => {
            let claim: JWTBody = claim;

            if claim.csrf_token == expected_csrf_token { Ok(claim.uid) }
            else { Err(()) }
        },
        Err(_) => Err(())
    }
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

pub async fn create_user(db: &NearsayDB, username: String, userhash: String) -> Result<AuthPair, NearsayError> {
    let uid = gen_id();

    match create_jwt_and_csrf(uid.clone()) {
        Ok(auth_pair) => {
            let user = User {
                _id: uid,
                username,
                hash: userhash,
            };

            match db.insert_user(user).await {
                Ok(_) => Ok(auth_pair),
                Err(err) => Err(err)
            }
        },
        Err(_) => Err(NearsayError::ServerError)
    }
}



pub async fn get_auth_info(db: &NearsayDB, username: String, userhash: String) -> Result<AuthPair, NearsayError> {
    match db.get_user(username).await {
        Ok(Some(user)) => {
            
            match bcrypt::verify(userhash, &user.hash[..]) {
                Ok(verified) => match verified {
                    true => match create_jwt_and_csrf(user._id) {
                        Ok(auth_pair) => Ok(auth_pair),
                        Err(_) => Err(NearsayError::ServerError),
                    },
                    false => Err(NearsayError::Unauthorized),
                },
                Err(bcrypt_err) => {
                    eprintln!("bcrypt error when authorizing user: {}", bcrypt_err);
                    Err(NearsayError::ServerError)
                },
            }
        }
        Ok(None) => Err(NearsayError::UserNotFound),
        Err(_) => Err(NearsayError::ServerError)
    }
}