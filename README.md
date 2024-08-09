# Tear

A bot that makes you cry.

---

Tear is an open-source edition of [Xeon](https://t.me/Intear_Xeon_bot), that currently has 4 features:

- Contract Logs (Text & NEP-297)
- Account Info
- Token Holders
- Near TGI

### Running

After cloning this repo, run `./setup.sh` (a workaround for [this issue](https://github.com/rust-lang/cargo/issues/4544)), and then `cargo run`.

You need to have `MAIN_TOKEN` environment variable that contains Telegram bot token that you can get from botfather.

### Architecture

The bot consists of multiple modules. The first and necessary one is HubModule. It handles /start and gives buttons that help users access other modules. There are also event handlers that handle events from blockchain, they are somewhat similar to modules, and one struct can implement both traits. Check out `tearbot/src/main.rs` to see how to register a module. Modules in Tear are hidden behind feature flags. Some of the modules are open-source, so these features are enabled by default, but some are stored inside `xeon-private-modules`, which are not accessible publicly. Tear can support multiple telegram bot instances, with different or shared per-bot data and modules, check out the `main.rs` to see more.

There are 2 types of telegram events: Callbacks (button clicks) and Messages. Telegram has a limit of 64 characters in button metadata, so when the bot sends a button with a callback data, it creates a hash and stores its base58 representation in MongoDB. When the user clicks a button, the bot pulls the data associated with this hash, deserializes it into enum `TgCommand`, and lets every module handle this callback. The bot works primarily with `TgContext` struct, which has a method `edit_or_send` that is quite convenient if you want to avoid sending a new message each time a user interacts with a button. The context stores a shared reference to struct `BotData`, which contains various internal bot data, such as connected accounts, message commands (message requests), etc. When the bot needs to request the user to send a message, it uses `bot_data.set_dm_message_command(user_id, MessageCommand::Something)`, which stores the `MessageCommand` in a `PersistentCachedStore<UserId, MessageCommand>` for later use. When a user sends a message, and the user exists in this data structure, the modules handle this `MessageCommand`, and if the message is valid and the action was successful, call `bot_data.remove_dm_message_command(UserId)`.

One struct you would notice particularly often in the codebase is `PersistentCachedStore`, it's a high-level abstraction over a MongoDB key-value store. Check the comments in the struct declaration to understand more about its usage.

### Event source

Some modules (for example, Contract Logs) get events from blockchain using an indexer. There are 2 event sources available:

1. WebSocket `wss://ws-events.intear.tech/`: An indexer hosted by Intear, the easiest and the default option.
2. Self-hosted [`all-indexers`](https://github.com/INTEARnear/all-indexers). To use this, install Redis, run `all-indexers` in the background, set `REDIS_URL` environment variable, and run the bot with `redis-events` feature.
