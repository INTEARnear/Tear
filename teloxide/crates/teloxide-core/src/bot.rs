use std::{future::Future, sync::Arc};

use reqwest::Client;
use serde::{de::DeserializeOwned, Serialize};

use crate::{
    net,
    requests::{MultipartPayload, Payload, ResponseResult},
    serde_multipart,
};

mod api;
mod download;

const TELOXIDE_TOKEN: &str = "TELOXIDE_TOKEN";

/// A requests sender.
///
/// This is the main type of the library, it allows to send requests to the
/// [Telegram Bot API] and download files.
///
/// ## TBA methods
///
/// All TBA methods are located in the [`Requester`] [`impl for Bot`]. This
/// allows for opt-in behaviours using requester [adaptors].
///
/// ```
/// # async {
/// use teloxide_core::prelude::*;
///
/// let bot = Bot::new("TOKEN");
/// dbg!(bot.get_me().await?);
/// # Ok::<_, teloxide_core::RequestError>(()) };
/// ```
///
/// [`Requester`]: crate::requests::Requester
/// [`impl for Bot`]: Bot#impl-Requester
/// [adaptors]: crate::adaptors
///
/// ## File download
///
/// In the similar way as with TBA methods, file downloading methods are located
/// in a trait — [`Download<'_>`]. See its documentation for more.
///
/// [`Download<'_>`]: crate::net::Download
///
/// ## Clone cost
///
/// `Bot::clone` is relatively cheap, so if you need to share `Bot`, it's
/// recommended to clone it, instead of wrapping it in [`Arc<_>`].
///
/// [`Arc`]: std::sync::Arc
/// [Telegram Bot API]: https://core.telegram.org/bots/api
#[must_use]
#[derive(Debug, Clone)]
pub struct Bot {
    token: Arc<str>,
    api_url: Arc<reqwest::Url>,
    client: Client,
}

/// Constructors
impl Bot {
    /// Creates a new `Bot` with the specified token and the default
    /// [http-client](reqwest::Client).
    ///
    /// # Panics
    ///
    /// If it cannot create [`reqwest::Client`].
    pub fn new<S>(token: S) -> Self
    where
        S: Into<String>,
    {
        let client = net::default_reqwest_settings().build().expect("Client creation failed");

        Self::with_client(token, client)
    }

    /// Creates a new `Bot` with the specified token and your
    /// [`reqwest::Client`].
    ///
    /// # Caution
    ///
    /// Your custom client might not be configured correctly to be able to work
    /// in long time durations, see [issue 223].
    ///
    /// [`reqwest::Client`]: https://docs.rs/reqwest/latest/reqwest/struct.Client.html
    /// [issue 223]: https://github.com/teloxide/teloxide/issues/223
    pub fn with_client<S>(token: S, client: Client) -> Self
    where
        S: Into<String>,
    {
        let token = Into::<String>::into(token).into();
        let api_url = Arc::new(
            reqwest::Url::parse(net::TELEGRAM_API_URL)
                .expect("Failed to parse default Telegram bot API url"),
        );

        Self { token, api_url, client }
    }

    /// Creates a new `Bot` with the `TELOXIDE_TOKEN` & `TELOXIDE_PROXY`
    /// environmental variables (a bot's token & a proxy) and the default
    /// [`reqwest::Client`].
    ///
    /// This function passes the value of `TELOXIDE_PROXY` into
    /// [`reqwest::Proxy::all`], if it exists, otherwise returns the default
    /// client.
    ///
    /// # Panics
    ///  - If cannot get the `TELOXIDE_TOKEN`  environmental variable.
    ///  - If it cannot create [`reqwest::Client`].
    ///
    /// [`reqwest::Client`]: https://docs.rs/reqwest/0.10.1/reqwest/struct.Client.html
    /// [`reqwest::Proxy::all`]: https://docs.rs/reqwest/latest/reqwest/struct.Proxy.html#method.all
    pub fn from_env() -> Self {
        Self::from_env_with_client(crate::net::client_from_env())
    }

    /// Creates a new `Bot` with the `TELOXIDE_TOKEN` environmental variable (a
    /// bot's token) and your [`reqwest::Client`].
    ///
    /// # Panics
    /// If cannot get the `TELOXIDE_TOKEN` environmental variable.
    ///
    /// # Caution
    /// Your custom client might not be configured correctly to be able to work
    /// in long time durations, see [issue 223].
    ///
    /// [`reqwest::Client`]: https://docs.rs/reqwest/0.10.1/reqwest/struct.Client.html
    /// [issue 223]: https://github.com/teloxide/teloxide/issues/223
    pub fn from_env_with_client(client: Client) -> Self {
        Self::with_client(get_env(TELOXIDE_TOKEN), client)
    }

    /// Sets a custom API URL.
    ///
    /// For example, you can run your own [Telegram bot API server][tbas] and
    /// set its URL using this method.
    ///
    /// [tbas]: https://github.com/tdlib/telegram-bot-api
    ///
    /// ## Examples
    ///
    /// ```
    /// use teloxide_core::{
    ///     requests::{Request, Requester},
    ///     Bot,
    /// };
    ///
    /// # async {
    /// let url = reqwest::Url::parse("https://localhost/tbas").unwrap();
    /// let bot = Bot::new("TOKEN").set_api_url(url);
    /// // From now all methods will use "https://localhost/tbas" as an API URL.
    /// bot.get_me().await
    /// # };
    /// ```
    ///
    /// ## Multi-instance behaviour
    ///
    /// This method only sets the url for one bot instace, older clones are
    /// unaffected.
    ///
    /// ```
    /// use teloxide_core::Bot;
    ///
    /// let bot = Bot::new("TOKEN");
    /// let bot2 = bot.clone();
    /// let bot = bot.set_api_url(reqwest::Url::parse("https://example.com/").unwrap());
    ///
    /// assert_eq!(bot.api_url().as_str(), "https://example.com/");
    /// assert_eq!(bot.clone().api_url().as_str(), "https://example.com/");
    /// assert_ne!(bot2.api_url().as_str(), "https://example.com/");
    /// ```
    pub fn set_api_url(mut self, url: reqwest::Url) -> Self {
        self.api_url = Arc::new(url);
        self
    }
}

/// Getters
impl Bot {
    /// Returns currently used token.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Returns currently used http-client.
    #[must_use]
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Returns currently used token API url.
    #[must_use]
    pub fn api_url(&self) -> reqwest::Url {
        reqwest::Url::clone(&*self.api_url)
    }
}

impl Bot {
    pub(crate) fn execute_json<P>(
        &self,
        payload: &P,
    ) -> impl Future<Output = ResponseResult<P::Output>> + 'static
    where
        P: Payload + Serialize,
        P::Output: DeserializeOwned + 'static,
    {
        let client = self.client.clone();
        let token = Arc::clone(&self.token);
        let api_url = Arc::clone(&self.api_url);

        let timeout_hint = payload.timeout_hint();
        let params = serde_json::to_vec(payload)
            // this `expect` should be ok since we don't write request those may trigger error here
            .expect("serialization of request to be infallible");

        // async move to capture client&token&api_url&params
        async move {
            net::request_json(
                &client,
                token.as_ref(),
                reqwest::Url::clone(&*api_url),
                P::NAME,
                params,
                timeout_hint,
            )
            .await
        }
    }

    pub(crate) fn execute_multipart<P>(
        &self,
        payload: &mut P,
    ) -> impl Future<Output = ResponseResult<P::Output>>
    where
        P: MultipartPayload + Serialize,
        P::Output: DeserializeOwned + 'static,
    {
        let client = self.client.clone();
        let token = Arc::clone(&self.token);
        let api_url = Arc::clone(&self.api_url);

        let timeout_hint = payload.timeout_hint();
        let params = serde_multipart::to_form(payload);

        // async move to capture client&token&api_url&params
        async move {
            let params = params?.await;
            net::request_multipart(
                &client,
                token.as_ref(),
                reqwest::Url::clone(&*api_url),
                P::NAME,
                params,
                timeout_hint,
            )
            .await
        }
    }

    pub(crate) fn execute_multipart_ref<P>(
        &self,
        payload: &P,
    ) -> impl Future<Output = ResponseResult<P::Output>>
    where
        P: MultipartPayload + Serialize,
        P::Output: DeserializeOwned + 'static,
    {
        let client = self.client.clone();
        let token = Arc::clone(&self.token);
        let api_url = self.api_url.clone();

        let timeout_hint = payload.timeout_hint();
        let params = serde_multipart::to_form_ref(payload);

        // async move to capture client&token&api_url&params
        async move {
            let params = params?.await;
            net::request_multipart(
                &client,
                token.as_ref(),
                reqwest::Url::clone(&*api_url),
                P::NAME,
                params,
                timeout_hint,
            )
            .await
        }
    }
}

fn get_env(env: &'static str) -> String {
    std::env::var(env).unwrap_or_else(|_| panic!("Cannot get the {env} env variable"))
}
