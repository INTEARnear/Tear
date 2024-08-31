# Tear

A bot that makes you cry.

---

Tear is an open-source edition of [Xeon](https://t.me/Intear_Xeon_bot), that currently has 5 features:

- Contract Logs (Text & NEP-297)
- Account Info
- Token Holders
- Near TGI
- AI Moderator

### Running

After cloning this repo, run `./setup.sh` (a workaround for [this issue](https://github.com/rust-lang/cargo/issues/4544)), and then `cargo run`.

You need to have `MAIN_TOKEN` environment variable that contains Telegram bot token that you can get from botfather.

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

- `OPENAI_API_KEY`: OpenAI API key
- `OPENAI_MODERATE_ASSISTANT_ID`: OpenAI assistant ID with the following prompt:
  ```
  You don't have the context or the previous conversation, but if you even slightly feel that a message can be useful in some context, you should moderate it as 'Good'.
  If you are unsure about a message and don't have the context to evaluate it, pass it as 'MoreContextNeeded'.
  If the content of the message is not allowed, but it could be a real person sending it without knowing the rules, it's 'Inform'.
  If you're pretty sure that a message is harmful, but it doesn't have an obvious intent to harm users, moderate it as 'Suspicious'.
  If a message is clearly something that is explicitly not allowed in the chat rules, moderate it as 'Harmful'.
  If a message includes 'spam' or 'scam' or anything that is not allowed as a literal word, but is not actually spam or scam, moderate it as 'MoreContextNeeded'. It maybe someone reporting spam or scam to admins by replying to the message, but you don't have the context to know that.
  Note that if something can be harmful, but is not explicitly mentioned in the rules, you should moderate it as 'MoreContextNeeded'.
  ```
  and json schema as in `ai-moderator/schema/moderate.schema.json`.
- `OPENAI_PROMPT_EDITOR_ASSISTANT_ID`: OpenAI assistant ID with the following prompt:
  ```
  You help users to configure their AI Moderator prompt. Your job is to rewrite the old prompt in accordance with the changes that the user requested. If possible, don't alter the parts that the user didn't ask to change.
  ```
  and json schema as in `ai-moderator/schema/prompt_editor.schema.json`.
- `OPENAI_PROMPT_EDITION_ASSISTANT_ID`: OpenAI assistant ID with the following prompt:
  ```
  Your job is to help AI Moderator refine its prompt. The AI Moderator has a prompt that helps it define what should or should not be banned. But if a user was flagged by mistake, you will come to help. Given the old prompt, message, and reasoning of the AI Moderator, craft from one to four ways to improve the prompt, with a short description (up to 20 characters) to present to the user. The description should be very simple, for example, "Allow <domain>" (if the reason is a link), or "Allow all links", or "Allow links to *.website.tld", "Allow @mentions", "Allow price talk", "Allow self promotion", "Allow /<command>", etc. They should come sorted from 1st - the most narrow one to the most wide restriction lift.

  Example 1: "/connect@Intear_Xeon_bot slimedrgn.tg", reasoning: "The message contains a "/connect" command, which could potentially be harmful, depending on the context"
  1. Allow /connect command
  2. Allow all slash-@Intear_Xeon_bot commands
  3. Allow all slash commands

  Example 2: "Hey, I launched a token: [here](https://app.ref.finance/#intel.tkn.near|near)", reasoning: "The message contains a link to app.ref.finance, which is not allowed by chat rules"
  1. Allow app.ref.finance links
  2. Allow *.ref.finance links
  3. Allow all links
  4. Allow self-promotion of tokens and allow links

  The AI Moderator can't flag for review, update its model, or do anything other than returning "Yes" or "No", so don't offer advice that should be applied to something other than the prompt. The modified prompt should (mostly) contain the old prompt, with some changes added / inserted / edited / removed / rephrased that reflect this exception from rules. Do NOT add "if relevant", "is related", "is safe", or "if context is provided" because the context is never provided, and you never know if something is relevant. It's your job to help AI Moderator know what is relevant and what isn't. Look at provided "Reasoning" to determine which aspect of the prompt to tweak.
  ```
  and json schema as in `ai-moderator/schema/prompt_edition.schema.json`.

### Architecture

The bot consists of multiple modules. The first and necessary one is HubModule. It handles /start and gives buttons that help users access other modules. There are also event handlers that handle events from blockchain, they are somewhat similar to modules, and one struct can implement both traits. Check out `tearbot/src/main.rs` to see how to register a module. Modules in Tear are hidden behind feature flags. Some of the modules are open-source, so these features are enabled by default, but some are stored inside `xeon-private-modules`, which are not accessible publicly. Tear can support multiple telegram bot instances, with different or shared per-bot data and modules, check out the `main.rs` to see more.

There are 2 types of telegram events: Callbacks (button clicks) and Messages. Telegram has a limit of 64 characters in button metadata, so when the bot sends a button with a callback data, it creates a hash and stores its base58 representation in MongoDB. When the user clicks a button, the bot pulls the data associated with this hash, deserializes it into enum `TgCommand`, and lets every module handle this callback. The bot works primarily with `TgContext` struct, which has a method `edit_or_send` that is quite convenient if you want to avoid sending a new message each time a user interacts with a button. The context stores a shared reference to struct `BotData`, which contains various internal bot data, such as connected accounts, message commands (message requests), etc. When the bot needs to request the user to send a message, it uses `bot_data.set_dm_message_command(user_id, MessageCommand::Something)`, which stores the `MessageCommand` in a `PersistentCachedStore<UserId, MessageCommand>` for later use. When a user sends a message, and the user exists in this data structure, the modules handle this `MessageCommand`, and if the message is valid and the action was successful, call `bot_data.remove_dm_message_command(UserId)`.

One struct you would notice particularly often in the codebase is `PersistentCachedStore`, it's a high-level abstraction over a MongoDB key-value store. Check the comments in the struct declaration to understand more about its usage.

### Event source

Some modules (for example, Contract Logs) get events from blockchain using an indexer. There are 2 event sources available:

1. WebSocket `wss://ws-events.intear.tech/`: An indexer hosted by Intear, the easiest and the default option.
2. Self-hosted [`all-indexers`](https://github.com/INTEARnear/all-indexers). To use this, install Redis, run `all-indexers` in the background, set `REDIS_URL` environment variable, and run the bot with `redis-events` feature.
