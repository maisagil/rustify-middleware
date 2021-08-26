//! Contains the [Endpoint] trait and supporting functions.
#[cfg(feature = "blocking")]
use crate::blocking::client::Client as BlockingClient;
use crate::{
    client::Client,
    enums::{RequestMethod, RequestType, ResponseType},
    errors::ClientError,
};
use async_trait::async_trait;
use bytes::Bytes;
use http::{Request, Response};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

/// Represents a generic wrapper that can be applied to [Endpoint] results.
///
/// Some APIs use a generic wrapper when returning responses that contains
/// information about the response and the actual response data in a subfield.
/// This trait allows implementing a generic wrapper which can be used with
/// [Endpoint::exec_wrap] to automatically wrap the [Endpoint::Result] in the
/// wrapper. The only requirement is that the [Wrapper::Value] must enclose
/// the [Endpoint::Result].
pub trait Wrapper: DeserializeOwned {
    type Value;
}

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
/// use rustify::clients::reqwest::Client;
/// use rustify::endpoint::Endpoint;
/// use rustify_derive::Endpoint;
/// use serde::Serialize;
///
/// #[derive(Debug, Endpoint, Serialize)]
/// #[endpoint(path = "my/endpoint")]
/// struct MyEndpoint {}
///
/// // Configure a client with a base URL of http://myapi.com
/// let client = Client::default("http://myapi.com");
///     
/// // Construct a new instance of our Endpoint
/// let endpoint = MyEndpoint {};
///
/// // Execute our Endpoint using the client
/// // This sends a GET request to http://myapi.com/my/endpoint
/// // It assumes an empty response
/// # tokio_test::block_on(async {
/// let result = endpoint.exec(&client).await;
/// # })
/// ```
#[async_trait]
pub trait Endpoint: Send + Sync + Serialize + Sized {
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
    fn path(&self) -> String;

    /// The HTTP method to be used when executing this Endpoint.
    fn method(&self) -> RequestMethod;

    /// Optional query parameters to add to the request
    fn query(&self) -> Vec<(String, Value)> {
        Vec::new()
    }

    /// Optional raw request data that will be sent instead of serializing the
    /// struct.
    fn data(&self) -> Option<Bytes> {
        None
    }

    /// Executes the Endpoint using the given [Client] and returns the
    /// deserialized [Endpoint::Result].
    async fn exec<C: Client>(&self, client: &C) -> Result<Option<Self::Result>, ClientError> {
        log::info!("Executing endpoint");

        let req = build(client.base(), self)?;
        let resp = exec(client, req).await?;
        crate::http::parse(Self::RESPONSE_BODY_TYPE, resp.body())
    }

    /// Executes the Endpoint using the given [Client] and [MiddleWare],
    /// returning the deserialized response as defined by [Endpoint::Result].
    async fn exec_mut<C: Client, M: MiddleWare>(
        &self,
        client: &C,
        middle: &M,
    ) -> Result<Option<Self::Result>, ClientError> {
        log::info!("Executing endpoint");

        let req = build_mut(client.base(), self, middle)?;
        let resp = exec_mut(client, self, req, middle).await?;
        crate::http::parse(Self::RESPONSE_BODY_TYPE, resp.body())
    }

    /// Executes the Endpoint using the given [Client] and returns the
    /// deserialized [Endpoint::Result] wrapped in a [Wrapper].
    async fn exec_wrap<C, W>(&self, client: &C) -> Result<Option<W>, ClientError>
    where
        C: Client,
        W: Wrapper<Value = Self::Result>,
    {
        log::info!("Executing endpoint");

        let req = build(client.base(), self)?;
        let resp = exec(client, req).await?;
        crate::http::parse(Self::RESPONSE_BODY_TYPE, resp.body())
    }

    /// Executes the Endpoint using the given [Client] and [MiddleWare],
    /// returning the deserialized [Endpoint::Result] wrapped in a [Wrapper].
    async fn exec_wrap_mut<C, M, W>(&self, client: &C, middle: &M) -> Result<Option<W>, ClientError>
    where
        C: Client,
        M: MiddleWare,
        W: Wrapper<Value = Self::Result>,
    {
        log::info!("Executing endpoint");

        let req = build_mut(client.base(), self, middle)?;
        let resp = exec_mut(client, self, req, middle).await?;
        crate::http::parse(Self::RESPONSE_BODY_TYPE, resp.body())
    }

    /// Executes the Endpoint using the given [Client], returning the raw
    /// response as a byte array.
    async fn exec_raw<C: Client>(&self, client: &C) -> Result<Bytes, ClientError> {
        log::info!("Executing endpoint");

        let req = build(client.base(), self)?;
        let resp = exec(client, req).await?;
        Ok(resp.body().clone())
    }

    /// Executes the Endpoint using the given [Client] and [MiddleWare],
    /// returning the raw response as a byte array.
    async fn exec_raw_mut<C: Client, M: MiddleWare>(
        &self,
        client: &C,
        middle: &M,
    ) -> Result<Bytes, ClientError> {
        log::info!("Executing endpoint");

        let req = build_mut(client.base(), self, middle)?;
        let resp = exec_mut(client, self, req, middle).await?;
        Ok(resp.body().clone())
    }

    /// Executes the Endpoint using the given [Client] and returns the
    /// deserialized [Endpoint::Result].
    #[cfg(feature = "blocking")]
    fn exec_block<C: BlockingClient>(
        &self,
        client: &C,
    ) -> Result<Option<Self::Result>, ClientError> {
        log::info!("Executing endpoint");

        let req = build(client.base(), self)?;
        let resp = exec_block(client, req)?;
        crate::http::parse(Self::RESPONSE_BODY_TYPE, resp.body())
    }

    /// Executes the Endpoint using the given [Client] and [MiddleWare],
    /// returning the deserialized response as defined by [Endpoint::Result].
    #[cfg(feature = "blocking")]
    fn exec_mut_block<C: BlockingClient, M: MiddleWare>(
        &self,
        client: &C,
        middle: &M,
    ) -> Result<Option<Self::Result>, ClientError> {
        log::info!("Executing endpoint");

        let req = build_mut(client.base(), self, middle)?;
        let resp = exec_mut_block(client, self, req, middle)?;
        crate::http::parse(Self::RESPONSE_BODY_TYPE, resp.body())
    }

    /// Executes the Endpoint using the given [Client] and returns the
    /// deserialized [Endpoint::Result] wrapped in a [Wrapper].
    #[cfg(feature = "blocking")]
    fn exec_wrap_block<C, W>(&self, client: &C) -> Result<Option<W>, ClientError>
    where
        C: BlockingClient,
        W: Wrapper<Value = Self::Result>,
    {
        log::info!("Executing endpoint");

        let req = build(client.base(), self)?;
        let resp = exec_block(client, req)?;
        crate::http::parse(Self::RESPONSE_BODY_TYPE, resp.body())
    }

    /// Executes the Endpoint using the given [Client] and [MiddleWare],
    /// returning the deserialized [Endpoint::Result] wrapped in a [Wrapper].
    #[cfg(feature = "blocking")]
    fn exec_wrap_mut_block<C, M, W>(&self, client: &C, middle: &M) -> Result<Option<W>, ClientError>
    where
        C: BlockingClient,
        M: MiddleWare,
        W: Wrapper<Value = Self::Result>,
    {
        log::info!("Executing endpoint");

        let req = build_mut(client.base(), self, middle)?;
        let resp = exec_mut_block(client, self, req, middle)?;
        crate::http::parse(Self::RESPONSE_BODY_TYPE, resp.body())
    }

    /// Executes the Endpoint using the given [Client], returning the raw
    /// response as a byte array.
    #[cfg(feature = "blocking")]
    fn exec_raw_block<C: BlockingClient>(&self, client: &C) -> Result<Bytes, ClientError> {
        log::info!("Executing endpoint");

        let req = build(client.base(), self)?;
        let resp = exec_block(client, req)?;
        Ok(resp.body().clone())
    }

    /// Executes the Endpoint using the given [Client] and [MiddleWare],
    /// returning the raw response as a byte array.
    #[cfg(feature = "blocking")]
    fn exec_raw_mut_block<C: BlockingClient, M: MiddleWare>(
        &self,
        client: &C,
        middle: &M,
    ) -> Result<Bytes, ClientError> {
        log::info!("Executing endpoint");

        let req = build_mut(client.base(), self, middle)?;
        let resp = exec_mut_block(client, self, req, middle)?;
        Ok(resp.body().clone())
    }
}

pub trait MiddleWare: Sync + Send {
    fn request<E: Endpoint>(
        &self,
        endpoint: &E,
        req: &mut Request<Bytes>,
    ) -> Result<(), ClientError>;
    fn response<E: Endpoint>(
        &self,
        endpoint: &E,
        resp: &mut Response<Bytes>,
    ) -> Result<(), ClientError>;
}

/// Builds a [Request] from the base URL path and [Endpoint]
fn build<E: Endpoint>(base: &str, endpoint: &E) -> Result<Request<Bytes>, ClientError> {
    crate::http::build_request(
        base,
        endpoint.path().as_str(),
        endpoint.method(),
        endpoint.query(),
        crate::http::build_body(endpoint, E::REQUEST_BODY_TYPE, endpoint.data())?,
    )
}

/// Builds a [Request] from the base URL path and [Endpoint]
fn build_mut<E: Endpoint, M: MiddleWare>(
    base: &str,
    endpoint: &E,
    middle: &M,
) -> Result<Request<Bytes>, ClientError> {
    let mut req = crate::http::build_request(
        base,
        endpoint.path().as_str(),
        endpoint.method(),
        endpoint.query(),
        crate::http::build_body(endpoint, E::REQUEST_BODY_TYPE, endpoint.data())?,
    )?;

    middle.request(endpoint, &mut req)?;
    Ok(req)
}

async fn exec<C: Client>(client: &C, req: Request<Bytes>) -> Result<Response<Bytes>, ClientError> {
    client.execute(req).await
}

async fn exec_mut<C: Client, E: Endpoint, M: MiddleWare>(
    client: &C,
    endpoint: &E,
    req: Request<Bytes>,
    middle: &M,
) -> Result<Response<Bytes>, ClientError> {
    let mut resp = client.execute(req).await?;
    middle.response(endpoint, &mut resp)?;
    Ok(resp)
}

#[cfg(feature = "blocking")]
fn exec_block<C: BlockingClient>(
    client: &C,
    req: Request<Bytes>,
) -> Result<Response<Bytes>, ClientError> {
    client.execute(req)
}

#[cfg(feature = "blocking")]
fn exec_mut_block<C: BlockingClient, E: Endpoint, M: MiddleWare>(
    client: &C,
    endpoint: &E,
    req: Request<Bytes>,
    middle: &M,
) -> Result<Response<Bytes>, ClientError> {
    let mut resp = client.execute(req)?;
    middle.response(endpoint, &mut resp)?;
    Ok(resp)
}
