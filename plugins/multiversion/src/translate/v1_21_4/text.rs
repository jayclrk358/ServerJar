//! Text components 1.21.5 → 1.21.4 (ViaBackwards' `ComponentRewriter1_21_5`).
//!
//! 1.21.5 renamed `hoverEvent`/`clickEvent` to `hover_event`/`click_event` and flattened their
//! contents; block entities store names and sign lines as NBT instead of JSON strings.

use crate::proto::buf::{Reader, Result, Writer};
use crate::proto::nbt::{self, Tag};

/// Components that 1.21.4 can't show in a hover item; dropped from show_item.
const NEW_ONLY_COMPONENTS: &[&str] = &[
    "tooltip_display", "potion_duration_scale", "weapon", "villager/variant", "wolf/variant", "wolf/collar", "fox/variant",
    "salmon/size", "parrot/variant", "tropical_fish/pattern", "tropical_fish/base_color", "tropical_fish/pattern_color",
    "mooshroom/variant", "rabbit/variant", "cow/variant", "pig/variant", "chicken/variant", "frog/variant", "horse/variant",
    "painting/variant", "llama/variant", "axolotl/variant", "cat/variant", "cat/collar", "sheep/color", "shulker/color",
    "blocks_attacks", "provides_trim_material", "break_sound", "wolf/sound_variant", "provides_banner_patterns",
    "instrument", "jukebox_playable",
];

fn strip_ns(s: &str) -> &str {
    s.strip_prefix("minecraft:").unwrap_or(s)
}

fn take(tag: &mut Tag, key: &str) -> Option<Tag> {
    tag.remove(key)
}

/// Rewrites a text component tag in place.
pub fn downgrade(tag: &mut Tag) {
    match tag {
        Tag::List(items) => items.iter_mut().for_each(downgrade),
        Tag::Compound(_) => downgrade_compound(tag),
        _ => {}
    }
}

fn downgrade_compound(tag: &mut Tag) {
    for key in ["with", "extra"] {
        if let Some(Tag::List(items)) = tag.get_mut(key) {
            items.iter_mut().for_each(downgrade);
        }
    }
    if let Some(mut hover) = take(tag, "hover_event") {
        downgrade_hover(&mut hover);
        tag.insert("hoverEvent", hover);
    }
    if let Some(mut click) = take(tag, "click_event") {
        downgrade_click(&mut click);
        tag.insert("clickEvent", click);
    }
}

fn downgrade_hover(hover: &mut Tag) {
    let Some(action) = hover.get("action").and_then(Tag::as_str).map(strip_ns).map(str::to_string) else { return };
    match action.as_str() {
        "show_text" => {
            if let Some(mut value) = take(hover, "value") {
                downgrade(&mut value);
                hover.insert("contents", value);
            }
        }
        "show_entity" => {
            let mut contents = Tag::compound();
            if let Some(mut name) = take(hover, "name") {
                downgrade(&mut name);
                contents.insert("name", name);
            }
            if let Some(Tag::String(id)) = take(hover, "id") {
                let id = match strip_ns(&id) {
                    "splash_potion" | "lingering_potion" => "minecraft:potion".to_string(),
                    _ => id,
                };
                contents.insert("type", Tag::String(id));
            }
            if let Some(uuid) = take(hover, "uuid") {
                contents.insert("id", uuid);
            }
            hover.insert("contents", contents);
        }
        "show_item" => {
            let mut contents = Tag::compound();
            if let Some(count) = take(hover, "count") {
                contents.insert("count", count);
            }
            if let Some(id) = take(hover, "id") {
                contents.insert("id", id);
            }
            if let Some(mut comps) = take(hover, "components") {
                downgrade_item_components(&mut comps);
                contents.insert("components", comps);
            }
            hover.insert("contents", contents);
        }
        _ => {}
    }
}

fn downgrade_click(click: &mut Tag) {
    let Some(action) = click.get("action").and_then(Tag::as_str).map(strip_ns).map(str::to_string) else { return };
    match action.as_str() {
        "open_url" => {
            if let Some(url) = take(click, "url") {
                click.insert("value", url);
            }
        }
        "change_page" => {
            let page = click.get("page").and_then(Tag::as_i32).unwrap_or(1);
            click.remove("page");
            click.insert("value", Tag::String(page.to_string()));
        }
        "run_command" => {
            if let Some(Tag::String(cmd)) = take(click, "command") {
                let cmd = if cmd.starts_with('/') { cmd } else { format!("/{cmd}") };
                click.insert("value", Tag::String(cmd));
            }
        }
        "suggest_command" => {
            if let Some(cmd) = take(click, "command") {
                click.insert("value", cmd);
            }
        }
        _ => {}
    }
}

fn find_key(comps: &Tag, name: &str) -> Option<String> {
    let Tag::Compound(entries) = comps else { return None };
    entries
        .iter()
        .map(|(k, _)| k)
        .find(|k| strip_ns(k) == name)
        .cloned()
}

/// Hover-item components (NBT form): 1.21.5's tooltip_display becomes per-component
/// `show_in_tooltip` flags, text becomes JSON strings, and 1.21.5-only components go away.
fn downgrade_item_components(comps: &mut Tag) {
    let mut hidden: Vec<String> = Vec::new();
    if let Some(key) = find_key(comps, "tooltip_display") {
        if let Some(Tag::List(list)) = comps.get(&key).and_then(|t| t.get("hidden_components")) {
            hidden = list.iter().filter_map(Tag::as_str).map(|s| strip_ns(s).to_string()).collect();
        }
    }
    let show = |name: &str| Tag::Byte(i8::from(!hidden.iter().any(|h| h == name)));
    for name in ["item_name", "custom_name"] {
        if let Some(key) = find_key(comps, name) {
            if let Some(t) = comps.get(&key).cloned() {
                comps.insert(&key, Tag::String(to_json(&t)));
            }
        }
    }
    if let Some(key) = find_key(comps, "lore") {
        if let Some(Tag::List(lines)) = comps.get(&key).cloned() {
            comps.insert(&key, Tag::List(lines.iter().map(|l| Tag::String(to_json(l))).collect()));
        }
    }
    if let Some(key) = find_key(comps, "attribute_modifiers") {
        if let Some(mods) = comps.remove(&key) {
            comps.insert("attribute_modifiers", Tag::Compound(vec![("modifiers".into(), mods), ("show_in_tooltip".into(), show("attribute_modifiers"))]));
        }
    }
    if let Some(key) = find_key(comps, "dyed_color") {
        if let Some(rgb) = comps.remove(&key) {
            comps.insert("dyed_color", Tag::Compound(vec![("rgb".into(), rgb), ("show_in_tooltip".into(), show("dyed_color"))]));
        }
    }
    for name in ["unbreakable", "trim"] {
        if let Some(key) = find_key(comps, name) {
            let flag = show(name);
            if let Some(t) = comps.get_mut(&key) {
                t.insert("show_in_tooltip", flag);
            }
        }
    }
    for name in ["can_place_on", "can_break"] {
        if let Some(key) = find_key(comps, name) {
            if let Some(preds) = comps.remove(&key) {
                comps.insert(name, Tag::Compound(vec![("predicates".into(), preds), ("show_in_tooltip".into(), show(name))]));
            }
        }
    }
    for name in ["enchantments", "stored_enchantments"] {
        if let Some(key) = find_key(comps, name) {
            if let Some(levels) = comps.remove(&key) {
                comps.insert(name, Tag::Compound(vec![("levels".into(), levels), ("show_in_tooltip".into(), show(name))]));
            }
        }
    }
    if let Tag::Compound(entries) = comps {
        entries.retain(|(k, _)| {
            let bare = strip_ns(k.trim_start_matches('!'));
            !NEW_ONLY_COMPONENTS.contains(&bare)
        });
    }
}

/// Reads one NBT text component, downgrades it and writes it back.
pub fn copy_component(r: &mut Reader, w: &mut Writer) -> Result<()> {
    let mut tag = nbt::read(r)?;
    downgrade(&mut tag);
    nbt::write(w, &tag);
    Ok(())
}

/// Bool-prefixed optional text component.
pub fn copy_opt_component(r: &mut Reader, w: &mut Writer) -> Result<()> {
    let present = r.bool()?;
    w.bool(present);
    if present {
        copy_component(r, w)?;
    }
    Ok(())
}

const BOOLEAN_KEYS: &[&str] = &["bold", "italic", "underlined", "strikethrough", "obfuscated", "interpret"];

/// Downgrades a component and serializes it as 1.21.4 JSON text (for block entity NBT).
pub fn to_json(tag: &Tag) -> String {
    let mut t = tag.clone();
    downgrade(&mut t);
    let mut out = String::new();
    json_value(&t, None, &mut out);
    out
}

fn json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn json_number(v: f64, out: &mut String) {
    if v.is_finite() {
        out.push_str(&v.to_string());
    } else {
        out.push('0');
    }
}

fn json_value(tag: &Tag, key: Option<&str>, out: &mut String) {
    match tag {
        Tag::End => out.push_str("null"),
        Tag::Byte(b) => {
            if key.is_some_and(|k| BOOLEAN_KEYS.contains(&k)) {
                out.push_str(if *b != 0 { "true" } else { "false" });
            } else {
                out.push_str(&b.to_string());
            }
        }
        Tag::Short(v) => out.push_str(&v.to_string()),
        Tag::Int(v) => out.push_str(&v.to_string()),
        Tag::Long(v) => out.push_str(&v.to_string()),
        Tag::Float(v) => json_number(f64::from(*v), out),
        Tag::Double(v) => json_number(*v, out),
        Tag::String(s) => json_string(s, out),
        Tag::ByteArray(a) => json_array(a.iter().map(|v| v.to_string()), out),
        Tag::IntArray(a) => json_array(a.iter().map(|v| v.to_string()), out),
        Tag::LongArray(a) => json_array(a.iter().map(|v| v.to_string()), out),
        Tag::List(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                json_value(item, None, out);
            }
            out.push(']');
        }
        Tag::Compound(entries) => {
            out.push('{');
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                json_string(k, out);
                out.push(':');
                json_value(v, Some(k), out);
            }
            out.push('}');
        }
    }
}

fn json_array(items: impl Iterator<Item = String>, out: &mut String) {
    out.push('[');
    for (i, item) in items.enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&item);
    }
    out.push(']');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &str) -> Tag {
        Tag::String(v.into())
    }

    #[test]
    fn click_and_hover_are_renamed() {
        let mut t = Tag::Compound(vec![
            ("text".into(), s("hi")),
            ("click_event".into(), Tag::Compound(vec![("action".into(), s("run_command")), ("command".into(), s("spawn"))])),
            ("hover_event".into(), Tag::Compound(vec![("action".into(), s("show_text")), ("value".into(), s("tip"))])),
        ]);
        downgrade(&mut t);
        assert!(t.get("click_event").is_none());
        assert_eq!(t.get("clickEvent").and_then(|c| c.get("value")).and_then(Tag::as_str), Some("/spawn"));
        assert_eq!(t.get("hoverEvent").and_then(|c| c.get("contents")).and_then(Tag::as_str), Some("tip"));
    }

    #[test]
    fn json_output() {
        let t = Tag::Compound(vec![("text".into(), s("a\"b")), ("bold".into(), Tag::Byte(1)), ("color".into(), s("red"))]);
        assert_eq!(to_json(&t), r#"{"text":"a\"b","bold":true,"color":"red"}"#);
        assert_eq!(to_json(&s("plain")), r#""plain""#);
    }
}
