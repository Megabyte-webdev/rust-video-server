use jsonwebtoken::{ decode, DecodingKey, Validation };
use serde::Deserialize;

// #[derive(Debug, Deserialize)]
// pub struct Claims {
//     pub sub: String,
//     pub exp: usize,
// }

// pub fn verify_jwt(token: &str, secret: &str) -> Result<Claims, String> {
//     decode::<Claims>(token, &DecodingKey::from_secret(secret.as_ref()), &Validation::default())
//         .map(|data| data.claims)
//         .map_err(|e| format!("JWT error: {:?}", e))
// }
