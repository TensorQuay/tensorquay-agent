use crate::GatewayError;
use axum::{
    Json,
    extract::{FromRequest, Request},
};
use serde::de::DeserializeOwned;

pub(crate) struct ApiJson<T>(pub T);

impl<S, T> FromRequest<S> for ApiJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned,
{
    type Rejection = GatewayError;
    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        Json::<T>::from_request(request, state)
            .await
            .map(|Json(value)| Self(value))
            .map_err(|error| {
                if error.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
                    GatewayError::BodyTooLarge
                } else {
                    GatewayError::BadRequest(
                        "request body does not match the documented JSON schema",
                    )
                }
            })
    }
}
