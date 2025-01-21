use axum::http::HeaderMap;
use hmac::{Hmac, Mac};
use jwt::{SignWithKey, VerifyWithKey, Error as JwtError};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug)]
struct AuthClaim {
    uid: String,
    csrf_token: String, 
}

/// if successful, returns `( jwt token, csrf token )`
pub fn create_jwt_and_csrf_token(uid: String) -> Result<(String, String), JwtError> {
    
    let key: Hmac<Sha256> = Hmac::new_from_slice(b"some-secret").unwrap();
    let csrf_token = Uuid::new_v4().to_string();
    let claims = AuthClaim {
        uid,
        csrf_token: csrf_token.clone(),
    };

    match claims.sign_with_key(&key) {
        Ok(token) => Ok((token, csrf_token)),
        Err(err) => {
            eprintln!("error creating jwt: {}", &err);
            Err(err)
        },
    }

}

/// if successful, returns `uid`
pub fn authenticate(req_headers: &HeaderMap) -> Result<String, ()> {
    let key: Hmac<Sha256> = Hmac::new_from_slice(b"some-secret").unwrap();

    let jwt_token = req_headers.get("cookie").unwrap().to_str().unwrap();
    let expected_csrf_token = req_headers.get("csrf-token").unwrap().to_str().unwrap();

    match jwt_token.verify_with_key(&key) {
        Ok(claim) => {
            let claim: AuthClaim = claim;

            if claim.csrf_token == expected_csrf_token { Ok(claim.uid) }
            else { Err(()) }
        },
        Err(_) => Err(())
    }
}