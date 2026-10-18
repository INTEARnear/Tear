/// Maximum length of a message text in Unicode code points after entity parsing
pub const MESSAGE_LENGTH_LIMIT: usize = 4096;
/// The Bot API rejects longer text parameters before parsing them
pub const TEXT_BYTES_LIMIT: usize = 1 << 15;

pub fn fits_in_message(markdown: &str) -> bool {
    markdown.len() <= TEXT_BYTES_LIMIT
        && markdown_length(markdown).map_or(true, |length| length <= MESSAGE_LENGTH_LIMIT)
}

fn markdown_length(markdown: &str) -> Result<usize, anyhow::Error> {
    Ok(counted_text(markdown)?.chars().count())
}

fn counted_text(markdown: &str) -> Result<String, anyhow::Error> {
    let (text, mut entities) = parse_markdown_v2(markdown.as_bytes())?;
    remove_unfixable_entities(&mut entities);
    let (text, skipped_before) = clean_input_string(&text);

    let Some(last_non_whitespace_pos) = text.iter().rposition(|&c| c != b' ' && c != b'\n') else {
        return Ok(String::new());
    };
    let text = &text[..=last_non_whitespace_pos];
    // entities that start after the last non-whitespace character are removed by rtrim
    let text_utf16_length = text.iter().map(|&c| utf16_length(c)).sum::<usize>();
    let first_entity_offset = entities
        .iter()
        .map(|entity| {
            (
                entity.offset - skipped_before[entity.offset],
                entity.end - skipped_before[entity.end],
            )
        })
        .filter(|&(offset, end)| offset < end && offset < text_utf16_length)
        .map(|(offset, _)| offset)
        .min()
        .unwrap_or(text.len());
    // ltrim doesn't cross the first entity
    let first_pos = text
        .iter()
        .take(first_entity_offset)
        .take_while(|&&c| c == b' ' || c == b'\n')
        .count();
    Ok(String::from_utf8(text[first_pos..].to_vec())?)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EntityType {
    Bold,
    Italic,
    Underline,
    Strikethrough,
    Spoiler,
    Code,
    Pre,
    TextUrl,
    CustomEmoji,
    BlockQuote,
}

impl EntityType {
    fn is_continuous(self) -> bool {
        matches!(
            self,
            EntityType::Code | EntityType::Pre | EntityType::TextUrl | EntityType::CustomEmoji
        )
    }
}

/// Offsets are in UTF-16 code units
#[derive(Clone, Copy)]
struct Entity {
    entity_type: EntityType,
    offset: usize,
    end: usize,
}

struct OpenEntity {
    entity_type: EntityType,
    offset: usize,
    byte_offset: usize,
    text_pos: usize,
}

/// Byte at `pos`, or 0 past the end like a NUL-terminated C++ string
fn at(text: &[u8], pos: usize) -> u8 {
    text.get(pos).copied().unwrap_or(0)
}

fn utf16_length(first_byte: u8) -> usize {
    match first_byte {
        0x80..=0xbf => 0,
        0xf0.. => 2,
        _ => 1,
    }
}

fn is_escapable(c: u8) -> bool {
    (1..=126).contains(&c)
}

fn reserved_character_error(c: u8) -> anyhow::Error {
    anyhow::anyhow!(
        "Character '{}' is reserved and must be escaped with the preceding '\\'",
        c as char
    )
}

fn unclosed_entity_error(entity: &OpenEntity) -> anyhow::Error {
    anyhow::anyhow!(
        "Can't find end of {:?} entity at byte offset {}",
        entity.entity_type,
        entity.byte_offset
    )
}

fn parse_markdown_v2(markdown: &[u8]) -> Result<(Vec<u8>, Vec<Entity>), anyhow::Error> {
    let mut text = Vec::with_capacity(markdown.len());
    let mut entities = Vec::new();
    let mut nested_entities: Vec<OpenEntity> = Vec::new();
    let mut utf16_offset = 0;
    let mut have_blockquote = false;
    let mut can_start_blockquote = true;
    // the text up to blank_prefix_length is removed or trimmed, if it contains a space or a newline
    let mut blank_prefix_length = 0;
    let mut last_blank_space_pos = None;
    let mut i = 0;
    while i < markdown.len() {
        let c = markdown[i];
        if c == b'\\' && is_escapable(at(markdown, i + 1)) {
            i += 1;
            utf16_offset += 1;
            text.push(markdown[i]);
            if markdown[i] != b'\r' {
                can_start_blockquote = markdown[i] == b'\n';
            }
            i += 1;
            continue;
        }

        let is_reserved = match nested_entities.last() {
            Some(entity) if matches!(entity.entity_type, EntityType::Code | EntityType::Pre) => {
                c == b'`'
            }
            _ => b"_*[]()~`>#+-=|{}.!\n".contains(&c),
        };
        if !is_reserved {
            if utf16_length(c) != 0 && c != b'\r' {
                can_start_blockquote = false;
            }
            utf16_offset += utf16_length(c);
            text.push(c);
            i += 1;
            continue;
        }

        let is_end_of_an_entity = match nested_entities.last() {
            None => false,
            Some(_) if have_blockquote && c == b'\n' && at(markdown, i + 1) != b'>' => true,
            Some(entity) => match entity.entity_type {
                EntityType::Bold => c == b'*',
                EntityType::Italic => c == b'_' && at(markdown, i + 1) != b'_',
                EntityType::Underline => c == b'_' && at(markdown, i + 1) == b'_',
                EntityType::Strikethrough => c == b'~',
                EntityType::Spoiler => c == b'|' && at(markdown, i + 1) == b'|',
                EntityType::Code => c == b'`',
                EntityType::Pre => {
                    c == b'`' && at(markdown, i + 1) == b'`' && at(markdown, i + 2) == b'`'
                }
                EntityType::TextUrl | EntityType::CustomEmoji => c == b']',
                EntityType::BlockQuote => false,
            },
        };
        if !is_end_of_an_entity {
            let byte_offset = i;
            let entity_type = match c {
                b'_' if at(markdown, i + 1) == b'_' => {
                    i += 1;
                    EntityType::Underline
                }
                b'_' => EntityType::Italic,
                b'*' => EntityType::Bold,
                b'~' => EntityType::Strikethrough,
                b'|' if at(markdown, i + 1) == b'|' => {
                    i += 1;
                    EntityType::Spoiler
                }
                b'[' => EntityType::TextUrl,
                b'`' if at(markdown, i + 1) == b'`' && at(markdown, i + 2) == b'`' => {
                    i += 3;
                    let mut language_end = i;
                    while !matches!(
                        at(markdown, language_end),
                        b' ' | b'\t' | b'\r' | b'\n' | 0 | 0x0b | b'`'
                    ) {
                        language_end += 1;
                    }
                    if i != language_end
                        && language_end < markdown.len()
                        && markdown[language_end] != b'`'
                    {
                        i = language_end;
                    }
                    // skip one new line in the beginning of the text
                    if matches!(at(markdown, i), b'\n' | b'\r') {
                        if matches!(at(markdown, i + 1), b'\n' | b'\r')
                            && at(markdown, i) != at(markdown, i + 1)
                        {
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                    i -= 1;
                    EntityType::Pre
                }
                b'`' => EntityType::Code,
                b'!' if at(markdown, i + 1) == b'[' => {
                    i += 1;
                    EntityType::CustomEmoji
                }
                b'\n' => {
                    utf16_offset += 1;
                    text.push(b'\n');
                    can_start_blockquote = true;
                    i += 1;
                    continue;
                }
                b'>' if can_start_blockquote => {
                    if have_blockquote {
                        i += 1;
                        continue;
                    }
                    have_blockquote = true;
                    EntityType::BlockQuote
                }
                _ => return Err(reserved_character_error(c)),
            };
            nested_entities.push(OpenEntity {
                entity_type,
                offset: utf16_offset,
                byte_offset,
                text_pos: text.len(),
            });
        } else {
            let mut entity = nested_entities.pop().unwrap();
            if c == b'\n' && entity.entity_type != EntityType::BlockQuote {
                // "||" right before the end of an expandable blockquote is parsed as a spoiler
                let is_expandable_blockquote_end = entity.entity_type == EntityType::Spoiler
                    && (entity.byte_offset + 2 == i
                        || (entity.byte_offset + 3 == i && text.last() == Some(&b'\r')));
                if !is_expandable_blockquote_end {
                    return Err(unclosed_entity_error(&entity));
                }
                entity = nested_entities.pop().unwrap();
                if entity.entity_type != EntityType::BlockQuote {
                    return Err(unclosed_entity_error(&entity));
                }
            }
            let mut skip_entity = utf16_offset == entity.offset;
            match entity.entity_type {
                EntityType::Bold
                | EntityType::Italic
                | EntityType::Strikethrough
                | EntityType::Code => {}
                EntityType::Underline | EntityType::Spoiler => i += 1,
                EntityType::Pre => i += 2,
                EntityType::TextUrl | EntityType::CustomEmoji => {
                    let mut url = None;
                    if at(markdown, i + 1) == b'(' {
                        i += 2;
                        let url_begin_pos = i;
                        let mut url_buffer = Vec::new();
                        while i < markdown.len() && markdown[i] != b')' {
                            if markdown[i] == b'\\' && is_escapable(at(markdown, i + 1)) {
                                i += 1;
                            }
                            url_buffer.push(markdown[i]);
                            i += 1;
                        }
                        if i == markdown.len() {
                            return Err(anyhow::anyhow!(
                                "Can't find end of a URL at byte offset {url_begin_pos}"
                            ));
                        }
                        url = Some(url_buffer);
                    } else if entity.entity_type == EntityType::CustomEmoji {
                        return Err(anyhow::anyhow!(
                            "Custom emoji entity must contain a tg://emoji or tg://time URL"
                        ));
                    }
                    // Telegram also rejects invalid tg://emoji and tg://time URLs, which doesn't
                    // change the length. A link without a valid URL isn't an entity, which only
                    // matters if it starts with trimmed whitespace.
                    while let Some(length) = blank_character_length(&text[blank_prefix_length..]) {
                        if length == 1 && text[blank_prefix_length] != b'\r' {
                            last_blank_space_pos = Some(blank_prefix_length);
                        }
                        blank_prefix_length += length;
                    }
                    if entity.entity_type == EntityType::TextUrl
                        && last_blank_space_pos.is_some_and(|pos| pos >= entity.text_pos)
                        // the link text is used as a URL if there is none
                        && !is_valid_link(url.as_deref().unwrap_or(&text[entity.text_pos..]))
                    {
                        skip_entity = true;
                    }
                }
                EntityType::BlockQuote => {
                    have_blockquote = false;
                    text.push(b'\n');
                    can_start_blockquote = true;
                    utf16_offset += 1;
                    skip_entity = false;
                }
            }
            if !skip_entity {
                entities.push(Entity {
                    entity_type: entity.entity_type,
                    offset: entity.offset,
                    end: utf16_offset,
                });
            }
        }
        i += 1;
    }
    if have_blockquote {
        if let Some(entity) = nested_entities.last()
            && entity.entity_type == EntityType::Spoiler
            && entity.byte_offset + 2 == markdown.len()
        {
            nested_entities.pop();
        }
        if let Some(entity) =
            nested_entities.pop_if(|entity| entity.entity_type == EntityType::BlockQuote)
            && entity.offset != utf16_offset
        {
            entities.push(Entity {
                entity_type: EntityType::BlockQuote,
                offset: entity.offset,
                end: utf16_offset,
            });
        }
    }
    if let Some(entity) = nested_entities.last() {
        return Err(unclosed_entity_error(entity));
    }
    Ok((text, entities))
}

/// fix_entities removes continuous entities that are nested in another continuous entity or
/// intersect a blockquote other than by lying inside it. Other entities are kept or split without
/// moving their first part further than a kept continuous entity starts.
fn remove_unfixable_entities(entities: &mut Vec<Entity>) {
    let mut continuous = entities
        .extract_if(.., |entity| entity.entity_type.is_continuous())
        .collect::<Vec<_>>();
    continuous.sort_by_key(|entity| (entity.offset, std::cmp::Reverse(entity.end)));
    let mut last_end = 0;
    continuous.retain(|entity| {
        let is_nested = entity.offset < last_end;
        if !is_nested {
            last_end = entity.end;
        }
        !is_nested
    });
    let mut blockquotes = entities
        .iter()
        .filter(|entity| entity.entity_type == EntityType::BlockQuote)
        .collect::<Vec<_>>();
    blockquotes.sort_by_key(|entity| entity.offset);
    let mut blockquotes = blockquotes.into_iter().peekable();
    continuous.retain(|entity| {
        while blockquotes
            .next_if(|blockquote| blockquote.end <= entity.offset)
            .is_some()
        {}
        blockquotes.peek().is_none_or(|blockquote| {
            entity.end <= blockquote.end
                && (blockquote.offset <= entity.offset || blockquote.offset >= entity.end)
        })
    });
    entities.extend(continuous);
}

/// Length of the character at the start of `text` if clean_input_string turns it into a space or a
/// newline or removes it
fn blank_character_length(text: &[u8]) -> Option<usize> {
    match text {
        [0..=32, ..] => Some(1),
        [0xe2, 0x80, 0xa8..=0xae, ..] => Some(3),
        [0xcc, 0xb3 | 0xbf | 0x8a, ..] => Some(2),
        _ => None,
    }
}

/// Removes control characters like clean_input_string_with_entities and returns the number of
/// removed UTF-16 code units before each UTF-16 offset of the input
fn clean_input_string(text: &[u8]) -> (Vec<u8>, Vec<usize>) {
    let mut result = Vec::with_capacity(text.len());
    let mut skipped_before = Vec::with_capacity(text.len() + 1);
    let mut skipped = 0;
    let mut pos = 0;
    while pos < text.len() {
        let c = text[pos];
        for _ in 0..utf16_length(c) {
            skipped_before.push(skipped);
        }
        match c {
            0..=9 | 11 | 12 | 14..=32 => result.push(b' '),
            b'\r' => skipped += 1,
            // U+2028..U+202E
            0xe2 if at(text, pos + 1) == 0x80 && (0xa8..=0xae).contains(&at(text, pos + 2)) => {
                pos += 2;
                skipped += 1;
            }
            // U+0333, U+033F, U+030A
            0xcc if matches!(at(text, pos + 1), 0xb3 | 0xbf | 0x8a) => {
                pos += 1;
                skipped += 1;
            }
            _ => result.push(c),
        }
        pos += 1;
    }
    skipped_before.push(skipped);
    (result, skipped_before)
}

/// Byte-wise case-insensitive prefix check
fn starts_with_ignore_case(text: &[u8], prefix: &[u8]) -> bool {
    text.get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

struct HttpUrl<'a> {
    is_https: bool,
    has_userinfo: bool,
    host: &'a [u8],
    is_ipv6: bool,
    has_port: bool,
}

/// td::parse_url
fn parse_url(url: &[u8]) -> Option<HttpUrl<'_>> {
    let protocol_end = url
        .iter()
        .position(|c| b":/?#@[]".contains(c))
        .unwrap_or(url.len());
    let (is_https, url) = match url[protocol_end..].strip_prefix(b"://") {
        Some(rest) if url[..protocol_end].eq_ignore_ascii_case(b"http") => (false, rest),
        Some(rest) if url[..protocol_end].eq_ignore_ascii_case(b"https") => (true, rest),
        Some(_) => return None,
        None => (false, url),
    };
    let userinfo_host_port = &url[..url
        .iter()
        .position(|c| b"/?#".contains(c))
        .unwrap_or(url.len())];

    let mut colon = userinfo_host_port.len().saturating_sub(1);
    while colon > 0 && !b":]@".contains(&userinfo_host_port[colon]) {
        colon -= 1;
    }
    let has_port = colon > 0 && userinfo_host_port[colon] == b':';
    let userinfo_host = if has_port {
        let mut port = &userinfo_host_port[colon + 1..];
        while port.len() > 1 && port[0] == b'0' {
            port = &port[1..];
        }
        // to_integer_safe accepts only the canonical form of a number
        let port = std::str::from_utf8(port).ok().and_then(|port| {
            port.parse::<i32>()
                .ok()
                .filter(|value| value.to_string() == port)
        });
        if !port.is_some_and(|port| (1..=65535).contains(&port)) {
            return None;
        }
        &userinfo_host_port[..colon]
    } else {
        userinfo_host_port
    };
    let (userinfo, host) = match userinfo_host.iter().rposition(|&c| c == b'@') {
        Some(at_pos) => (&userinfo_host[..at_pos], &userinfo_host[at_pos + 1..]),
        None => (&b""[..], userinfo_host),
    };

    let is_ipv6 = host.first() == Some(&b'[') && host.last() == Some(&b']');
    if is_ipv6
        && (host.len() <= 2
            || std::str::from_utf8(&host[1..host.len() - 1])
                .ok()
                .and_then(|ip| ip.parse::<std::net::Ipv6Addr>().ok())
                .is_none())
    {
        return None;
    }
    if host.is_empty() || host == b"." {
        return None;
    }
    let is_valid_url_part = |part: &[u8], allow_colon: bool| {
        let mut i = 0;
        while i < part.len() {
            let c = part[i];
            if c == b'%' {
                if !at(part, i + 1).is_ascii_hexdigit() || !at(part, i + 2).is_ascii_hexdigit() {
                    return false;
                }
                i += 2;
            } else if !(c.is_ascii_alphanumeric()
                || b".-_!$,~*'();&+=".contains(&c)
                || (allow_colon && c == b':')
                || c >= 128)
            {
                return false;
            }
            i += 1;
        }
        true
    };
    let is_valid_host = if is_ipv6 {
        host[1..host.len() - 1]
            .iter()
            .all(|&c| c == b':' || c == b'.' || c.is_ascii_hexdigit())
    } else {
        is_valid_url_part(host, false) && is_valid_url_part(userinfo, true)
    };
    is_valid_host.then_some(HttpUrl {
        is_https,
        has_userinfo: !userinfo.is_empty(),
        host,
        is_ipv6,
        has_port,
    })
}

/// LinkManager::check_link. Every tg://user link that get_link_user_id accepts is valid too.
fn is_valid_link(url: &[u8]) -> bool {
    let scheme = [&b"tg:"[..], b"ton:", b"tonsite:"]
        .into_iter()
        .find(|scheme| starts_with_ignore_case(url, scheme));
    let link = match scheme {
        Some(scheme) => {
            let link = &url[scheme.len()..];
            link.strip_prefix(b"//").unwrap_or(link)
        }
        None => url,
    };
    let Some(http_url) = parse_url(link) else {
        return false;
    };
    match scheme {
        Some(scheme) => {
            !starts_with_ignore_case(link, b"http://")
                && !http_url.is_https
                && !http_url.has_userinfo
                && !http_url.has_port
                && !http_url.is_ipv6
                && http_url.host.iter().all(|&c| {
                    c.is_ascii_alphanumeric()
                        || c == b'-'
                        || c == b'_'
                        || (scheme == b"tonsite:" && c == b'.')
                })
        }
        None => http_url.is_ipv6 || http_url.host.contains(&b'.'),
    }
}
