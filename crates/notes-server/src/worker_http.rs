//! Worker adapter: routing, cookies and JSON, all above [`crate::Server`].

use std::rc::Rc;

use notes_proto::{
    envelope, AuthRequest, BlobChunk, CredentialUpdateRequest, Envelope, ErrorBody, SyncRequest,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use worker::{js_sys, Env, Request, Response, RouteContext, Router};

use crate::d1::D1Storage;
use crate::{Server, ServerConfig, ServerError};

/// Cookie name used by the web client. Native clients send a bearer token.
pub const SESSION_COOKIE: &str = "notebox_session";

/// Header that carries the one-time setup token.
pub const SETUP_TOKEN_HEADER: &str = "x-setup-token";

#[derive(Clone)]
struct AppState {
    server: Rc<Server<D1Storage>>,
}

impl AppState {
    fn from_env(env: &Env) -> Result<Self, ServerError> {
        let sync = env
            .d1("DB_SYNC")
            .map_err(|error| ServerError::Storage(error.to_string()))?;
        let blobs = env
            .d1("DB_BLOBS")
            .map_err(|error| ServerError::Storage(error.to_string()))?;
        let setup_token = env
            .secret("SETUP_TOKEN")
            .ok()
            .map(|value| value.to_string())
            .filter(|value| !value.is_empty());
        let config = match setup_token {
            Some(token) => ServerConfig::with_setup_token(token),
            None => ServerConfig::without_setup(),
        };
        Ok(Self {
            server: Rc::new(Server::new(D1Storage::new(sync, blobs), config)),
        })
    }
}

/// Entry point called by the `#[event(fetch)]` wrapper in `lib.rs`.
pub async fn handle(req: Request, env: Env) -> worker::Result<Response> {
    let state = match AppState::from_env(&env) {
        Ok(state) => state,
        Err(error) => return error_response(error),
    };
    Router::with_data(state)
        .get_async("/", health)
        .post_async("/v1/auth/setup", setup)
        .post_async("/v1/auth/login", login)
        .post_async("/v1/auth/credentials", update_credentials)
        .post_async("/v1/auth/logout", logout)
        .post_async("/v1/sync", sync)
        .put_async("/v1/blob/:hash", put_blob)
        .get_async("/v1/blob/:hash", get_blob)
        .get_async("/v1/devices", list_devices)
        .delete_async("/v1/devices/:id", revoke_device)
        .run(req, env)
        .await
}

/// `GET /` answers without touching storage, for deploy checks.
async fn health(_req: Request, _ctx: RouteContext<AppState>) -> worker::Result<Response> {
    Response::ok("notebox")
}

async fn setup(mut req: Request, ctx: RouteContext<AppState>) -> worker::Result<Response> {
    let result = async {
        let provided = header(&req, SETUP_TOKEN_HEADER);
        let request: AuthRequest = read_body(&mut req).await?;
        let response = ctx
            .data
            .server
            .setup(provided.as_deref(), &request, now_secs())
            .await?;
        Ok::<_, ServerError>(response)
    }
    .await;
    match result {
        Ok(response) => auth_response(&req, response),
        Err(error) => error_response(error),
    }
}

async fn login(mut req: Request, ctx: RouteContext<AppState>) -> worker::Result<Response> {
    let result = async {
        let request: AuthRequest = read_body(&mut req).await?;
        let response = ctx.data.server.login(&request, now_secs()).await?;
        Ok::<_, ServerError>(response)
    }
    .await;
    match result {
        Ok(response) => auth_response(&req, response),
        Err(error) => error_response(error),
    }
}

async fn update_credentials(
    mut req: Request,
    ctx: RouteContext<AppState>,
) -> worker::Result<Response> {
    let result = async {
        let device = authenticate(&req, &ctx.data.server).await?;
        let request: CredentialUpdateRequest = read_body(&mut req).await?;
        let response = ctx
            .data
            .server
            .update_credentials(&device, &request)
            .await?;
        Ok::<_, ServerError>(response)
    }
    .await;
    match result {
        Ok(response) => json_response(&envelope(response), 200),
        Err(error) => error_response(error),
    }
}

async fn logout(req: Request, _ctx: RouteContext<AppState>) -> worker::Result<Response> {
    // The cookie is the only browser credential. Clearing it here is what
    // makes Sign out stick across a reload; the bearer token (native clients
    // and the web client's in-memory copy) is forgotten by the caller.
    let secure = req.url().map(|url| url.scheme() == "https").unwrap_or(true);
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{SESSION_COOKIE}=; HttpOnly{}; SameSite=Strict; Path=/; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
        secure
    );
    let mut response = Response::empty()?.with_status(204);
    response.headers_mut().set("set-cookie", &cookie)?;
    Ok(response)
}

async fn sync(mut req: Request, ctx: RouteContext<AppState>) -> worker::Result<Response> {
    let result = async {
        let device = authenticate(&req, &ctx.data.server).await?;
        let request: SyncRequest = read_body(&mut req).await?;
        let response = ctx.data.server.sync(&device, &request, now_secs()).await?;
        Ok::<_, ServerError>(response)
    }
    .await;
    match result {
        Ok(response) => json_response(&envelope(response), 200),
        Err(error) => error_response(error),
    }
}

async fn put_blob(mut req: Request, ctx: RouteContext<AppState>) -> worker::Result<Response> {
    let result = async {
        let _device = authenticate(&req, &ctx.data.server).await?;
        let chunk: BlobChunk = read_body(&mut req).await?;
        let hash = ctx
            .param("hash")
            .cloned()
            .ok_or_else(|| ServerError::Invalid("missing hash".into()))?;
        if chunk.hash != hash {
            return Err(ServerError::Invalid(
                "path hash and body hash differ".into(),
            ));
        }
        let stored = ctx.data.server.put_blob(&chunk, now_secs()).await?;
        Ok::<_, ServerError>(stored)
    }
    .await;
    match result {
        Ok(stored) => json_response(&envelope(stored), 200),
        Err(error) => error_response(error),
    }
}

async fn get_blob(req: Request, ctx: RouteContext<AppState>) -> worker::Result<Response> {
    let result = async {
        let _device = authenticate(&req, &ctx.data.server).await?;
        let hash = ctx
            .param("hash")
            .cloned()
            .ok_or_else(|| ServerError::Invalid("missing hash".into()))?;
        let index = query_index(&req)?;
        let chunk = ctx.data.server.get_blob(&hash, index).await?;
        Ok::<_, ServerError>(chunk)
    }
    .await;
    match result {
        Ok(chunk) => json_response(&envelope(chunk), 200),
        Err(error) => error_response(error),
    }
}

async fn list_devices(req: Request, ctx: RouteContext<AppState>) -> worker::Result<Response> {
    let result = async {
        let device = authenticate(&req, &ctx.data.server).await?;
        let devices = ctx.data.server.list_devices(&device.id).await?;
        Ok::<_, ServerError>(devices)
    }
    .await;
    match result {
        Ok(devices) => json_response(&envelope(devices), 200),
        Err(error) => error_response(error),
    }
}

async fn revoke_device(req: Request, ctx: RouteContext<AppState>) -> worker::Result<Response> {
    let result = async {
        let _device = authenticate(&req, &ctx.data.server).await?;
        let id = ctx
            .param("id")
            .cloned()
            .ok_or_else(|| ServerError::Invalid("missing device id".into()))?;
        if !ctx.data.server.revoke_device(&id).await? {
            return Err(ServerError::NotFound("device".into()));
        }
        Ok::<_, ServerError>(())
    }
    .await;
    match result {
        Ok(()) => Response::empty().map(|response| response.with_status(204)),
        Err(error) => error_response(error),
    }
}

async fn authenticate(
    req: &Request,
    server: &Server<D1Storage>,
) -> Result<crate::DeviceRow, ServerError> {
    let token = request_token(req).ok_or(ServerError::Unauthorized)?;
    server.authenticate(&token, now_secs()).await
}

fn request_token(req: &Request) -> Option<String> {
    if let Ok(Some(value)) = req.headers().get("authorization") {
        if let Some(token) = value.strip_prefix("Bearer ") {
            let token = token.trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    let cookies = req.headers().get("cookie").ok().flatten()?;
    let prefix = format!("{SESSION_COOKIE}=");
    cookies
        .split(';')
        .map(str::trim)
        .find_map(|part| part.strip_prefix(&prefix))
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn header(req: &Request, name: &str) -> Option<String> {
    req.headers()
        .get(name)
        .ok()
        .flatten()
        .filter(|value| !value.is_empty())
}

fn query_index(req: &Request) -> Result<u32, ServerError> {
    let url = req
        .url()
        .map_err(|error| ServerError::Invalid(format!("invalid URL: {error}")))?;
    let value = url
        .query_pairs()
        .find(|(key, _)| key == "index")
        .map(|(_, value)| value.into_owned())
        .ok_or_else(|| ServerError::Invalid("missing index query parameter".into()))?;
    value
        .parse()
        .map_err(|_| ServerError::Invalid("index must be a number".into()))
}

async fn read_body<T: DeserializeOwned>(req: &mut Request) -> Result<T, ServerError> {
    let envelope: Envelope<T> = req
        .json()
        .await
        .map_err(|_| ServerError::Invalid("body is not a valid envelope".into()))?;
    if !envelope.is_current() {
        return Err(ServerError::Invalid(format!(
            "unsupported protocol version {}",
            envelope.version
        )));
    }
    Ok(envelope.payload)
}

fn auth_response(req: &Request, response: notes_proto::AuthResponse) -> worker::Result<Response> {
    // `Secure` is required in production. Local `wrangler dev` runs over plain
    // http, where a Secure cookie would never be stored; leaving the flag off
    // there keeps the browser session testable without weakening HTTPS.
    let secure = req.url().map(|url| url.scheme() == "https").unwrap_or(true);
    let secure = if secure { "; Secure" } else { "" };
    let cookie = format!(
        "{SESSION_COOKIE}={}; HttpOnly{}; SameSite=Strict; Path=/; Max-Age=31536000",
        response.token, secure
    );
    let mut built = json_response(&envelope(response), 200)?;
    built.headers_mut().set("set-cookie", &cookie)?;
    // The token stays in the JSON body for native clients; the web client can
    // rely on the HttpOnly cookie instead of reading it from script.
    Ok(built)
}

fn json_response<T: Serialize>(value: &T, status: u16) -> worker::Result<Response> {
    let mut response = Response::from_json(value)?.with_status(status);
    response.headers_mut().set("cache-control", "no-store")?;
    Ok(response)
}

fn error_response(error: ServerError) -> worker::Result<Response> {
    let body = ErrorBody {
        code: error.code().to_string(),
        message: error.to_string(),
        retry_after: error.retry_after(),
    };
    let mut response = json_response(&envelope(body), error.status())?;
    if let Some(seconds) = error.retry_after() {
        response
            .headers_mut()
            .set("retry-after", &seconds.to_string())?;
    }
    Ok(response)
}

fn now_secs() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}
