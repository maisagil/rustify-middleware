use crate::{
    client::{Client, Request, Response},
    enums::{RequestMethod, RequestType, ResponseType},
    errors::ClientError,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::fmt::Debug;
use url::Url;

/// Represents a remote HTTP endpoint which can be executed using a
/// [crate::client::Client].
///
/// This trait can be implemented directly, however, users should prefer using
/// the provided `rustify_derive` macro for generating implementations. An
/// Endpoint consists of:
///   * An `action` which is combined with the base URL of a Client to form a
///     fully qualified URL.
///   * A `method` of type [RequestType] which determines the HTTP method used
///     when a Client executes this endpoint.
///   * A `ResponseType` type which determines the type of response this
///     Endpoint will return when executed.
///
/// Presently, this trait only supports sending and receiving data using JSON.
/// The struct implementing this trait must also implement [serde::Serialize].
/// The fields of the struct act as a representation of data that will be
/// serialized and sent to the remote server. Fields that should be excluded
/// from this behavior can be tagged with the `#[serde(skip)]` attribute. The
/// Endpoint will take the raw response body from the remote server and attempt
/// to deserialize it into the given `ResponseType` which must implement
/// [serde::Deserialize]. This deserialized value is then returned after
/// execution completes.
///
/// Implementations can override the default [transform][Endpoint::transform] in
/// order to modify the raw response content from the remote server before
/// returning it. This is often useful when the remote API wraps all responses
/// in a common format and the desire is to remove the wrapper before returning
/// the deserialized response. It can also be used to check for any errors
/// generated by the API and escalate them accordingly.
///
/// # Example
/// ```
/// use rustify::clients::reqwest::ReqwestClient;
/// use rustify::endpoint::Endpoint;
/// use rustify_derive::Endpoint;
/// use serde::Serialize;
///
/// #[derive(Debug, Endpoint, Serialize)]
/// #[endpoint(path = "my/endpoint")]
/// struct MyEndpoint {}
///
/// // Configure a client with a base URL of http://myapi.com
/// let client = ReqwestClient::default("http://myapi.com");
///     
/// // Construct a new instance of our Endpoint
/// let endpoint = MyEndpoint {};
///
/// // Execute our Endpoint using the client
/// // This sends a GET request to http://myapi.com/my/endpoint
/// // It assumes an empty response
/// let result = endpoint.execute(&client);
/// ```
pub trait Endpoint: Debug + Serialize + Sized {
    /// The type that the raw response from executing this endpoint will
    /// automatically be deserialized to. This type must implement
    /// [serde::Deserialize].
    type Result: DeserializeOwned;

    /// The content type of the request body
    const REQUEST_BODY_TYPE: RequestType;

    /// The content type of the response body
    const RESPONSE_BODY_TYPE: ResponseType;

    /// The relative URL path that represents the location of this Endpoint.
    /// This is combined with the base URL from a
    /// [Client][crate::client::Client] instance to create the fully qualified
    /// URL.
    fn action(&self) -> String;

    /// The HTTP method to be used when executing this Endpoint.
    fn method(&self) -> RequestMethod;

    /// Optional query parameters to add to the request
    fn query(&self) -> Vec<(String, Value)> {
        Vec::new()
    }

    /// Executes the Endpoint using the given [Client] and returns the
    /// deserialized response as defined by [Endpoint::Result].
    fn execute<C: Client>(&self, client: &C) -> Result<Option<Self::Result>, ClientError> {
        log::info!("Executing endpoint");
        log::debug! {"Endpoint: {:#?}", self};

        let req = build_request(self, client.base())?;
        let resp = client.execute(req)?;
        parse(self, &resp.content)
    }

    /// Executes the Endpoint using the given [Client] and [MiddleWare],
    /// returning the deserialized response as defined by [Endpoint::Result].
    fn execute_m<C: Client, M: MiddleWare>(
        &self,
        client: &C,
        middle: &M,
    ) -> Result<Option<Self::Result>, ClientError> {
        log::info!("Executing endpoint");
        log::debug! {"Endpoint: {:#?}", self};

        let mut req = build_request(self, client.base())?;
        middle.request(self, &mut req)?;

        let mut resp = client.execute(req)?;
        middle.response(self, &mut resp)?;
        parse(self, &resp.content)
    }
}

pub trait MiddleWare {
    fn request<E: Endpoint>(&self, endpoint: &E, req: &mut Request) -> Result<(), ClientError>;
    fn response<E: Endpoint>(&self, endpoint: &E, resp: &mut Response) -> Result<(), ClientError>;
}

/// Builds a [Request] using the given [Endpoint] and base URL
fn build_request<E: Endpoint>(endpoint: &E, base: &str) -> Result<Request, ClientError> {
    let url = build_url(endpoint, base)?;
    let method = endpoint.method();
    let query = endpoint.query();
    let headers = Vec::new();
    let body = match E::REQUEST_BODY_TYPE {
        RequestType::JSON => {
            let parse_data =
                serde_json::to_string(endpoint).map_err(|e| ClientError::DataParseError {
                    source: Box::new(e),
                })?;
            match parse_data.as_str() {
                "null" => "".to_string(),
                "{}" => "".to_string(),
                _ => parse_data,
            }
            .into_bytes()
        }
    };

    Ok(Request {
        url,
        method,
        query,
        headers,
        body,
    })
}

/// Combines the given base URL with the relative URL path from this
/// Endpoint to create a fully qualified URL.
fn build_url<E: Endpoint>(endpoint: &E, base: &str) -> Result<url::Url, ClientError> {
    log::info!(
        "Building endpoint url from {} base URL and {} action",
        base,
        endpoint.action()
    );

    let mut url = Url::parse(base).map_err(|e| ClientError::UrlParseError {
        url: base.to_string(),
        source: e,
    })?;
    url.path_segments_mut()
        .unwrap()
        .extend(endpoint.action().split('/'));
    Ok(url)
}

/// Parses a response body into the [Endpoint::Result], choosing a deserializer
/// based on [Endpoint::RESPONSE_BODY_TYPE].
fn parse<E: Endpoint>(_: &E, body: &[u8]) -> Result<Option<E::Result>, ClientError> {
    if body.is_empty() {
        return Ok(None);
    }

    match E::RESPONSE_BODY_TYPE {
        ResponseType::JSON => {
            serde_json::from_slice(body).map_err(|e| ClientError::ResponseParseError {
                source: Box::new(e),
                content: String::from_utf8(body.to_vec()).ok(),
            })
        }
    }
}
