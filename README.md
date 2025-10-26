# Tear

A bot that makes you cry.

---

Tear is an open-source edition of [Xeon](https://t.me/Intear_Xeon_bot), that currently has 9 features:

- Contract Logs (Text & NEP-297)
- Account Info
- Token Holders
- Near TGI
- Moderator
- Burrow Liquidations
- House of Stake Notifications
- Price Commands
- Tip Bot

### Running

After cloning this repo, run `./setup.sh` (a workaround for [this issue](https://github.com/rust-lang/cargo/issues/4544)), and then `cargo run`.

You need to have `MAIN_TOKEN` environment variable that contains Telegram bot token that you can get from botfather.

### Test Server

If you want to use Telegram's test server, use the following Nginx configuration to proxy requests to the test server:

```nginx
server {
    listen 5555;
    server_name localhost;

    location ~ ^/bot([^/]+)/(.*) {
        set $token $1;
        set $method_name $2;

        proxy_pass https://api.telegram.org/bot$token/test/$method_name;
        proxy_pass_request_body on; 
        proxy_pass_request_headers on;       
    }

    location ~ ^/file/([^/]+)/(.*) {
        set $token $1;
        set $method_name $2;

        proxy_pass https://api.telegram.org/file/$token/test/$method_name;
        proxy_pass_request_body on;
        proxy_pass_request_headers on;
    }
}
```

#### Contract Logs

No additional setup is required.

#### Account Info

No additional setup is required.

#### Token Holders

No additional setup is required.

#### Near TGI

No additional setup is required.

#### AI Moderator

You need to have these environment variables:

- `OPENAI_API_KEY`: OpenAI API key (if you plan to use GPT-4o and GPT-4o-mini)
- `CEREBRAS_API_KEY`: Cerebras API key (if you plan to use Llama 70B). It will fall back to GPT-4o if the message contains an image, as this version of Llama is not multimodal.

#### Burrow Liquidations

No additional setup is required.

#### House of Stake Notifications

No additional setup is required.

#### Price Command

No additional setup is required.

#### Chart Command

No additional setup is required.

Also, you need to have `geckodriver` installed and in your PATH.

#### AI Agents

You need to have `NEAR_AI_API_KEY` and `BITTE_API_KEY` environment variables set. Get `NEAR_AI_API_KEY` from Near AI CLI (`pip install nearai`, `nearai login`, `cat ~/.nearai/config.json | jq .auth`) and `BITTE_API_KEY` from [Bitte](https://key.bitte.ai).

### Architecture

The bot consists of multiple modules. The first and necessary one is HubModule. It handles /start and gives buttons that help users access other modules. There are also event handlers that handle events from blockchain, they are somewhat similar to modules, and one struct can implement both traits. Check out `tearbot/src/main.rs` to see how to register a module. Modules in Tear are hidden behind feature flags. Some of the modules are open-source, so these features are enabled by default, but some are stored inside `xeon-private-modules`, which are not accessible publicly. Tear can support multiple telegram bot instances, with different or shared per-bot data and modules, check out the `main.rs` to see more.

There are 2 types of telegram events: Callbacks (button clicks) and Messages. Telegram has a limit of 64 characters in button metadata, so when the bot sends a button with a callback data, it creates a hash and stores its base58 representation in MongoDB. When the user clicks a button, the bot pulls the data associated with this hash, deserializes it into enum `TgCommand`, and lets every module handle this callback. The bot works primarily with `TgContext` struct, which has a method `edit_or_send` that is quite convenient if you want to avoid sending a new message each time a user interacts with a button. The context stores a shared reference to struct `BotData`, which contains various internal bot data, such as connected accounts, message commands (message requests), etc. When the bot needs to request the user to send a message, it uses `bot_data.set_dm_message_command(user_id, MessageCommand::Something)`, which stores the `MessageCommand` in a `PersistentCachedStore<UserId, MessageCommand>` for later use. When a user sends a message, and the user exists in this data structure, the modules handle this `MessageCommand`, and if the message is valid and the action was successful, call `bot_data.remove_dm_message_command(UserId)`.

One struct you would notice particularly often in the codebase is `PersistentCachedStore`, it's a high-level abstraction over a MongoDB key-value store. Check the comments in the struct declaration to understand more about its usage.

### Event source

Some modules (for example, Contract Logs) get events from blockchain using an indexer. There are 2 event sources available:

1. WebSocket `wss://ws-events-v3.intear.tech/`: An indexer hosted by Intear, the easiest and the default option.
2. Self-hosted [`all-indexers`](https://github.com/INTEARnear/all-indexers). To use this, install Redis, run `all-indexers` in the background, set `REDIS_URL` and `REDIS_URL_TESTNET` environment variables, and run the bot with `redis-events` feature.
