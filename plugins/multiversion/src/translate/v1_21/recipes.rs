//! Recipes 1.21.2 → 1.21, following ViaBackwards' `RecipeStorage`.
//!
//! 1.21.2 servers only send recipe *displays* for unlocked recipes (RECIPE_BOOK_ADD) plus the
//! stonecutter list (UPDATE_RECIPES). 1.21 clients expect full recipes in UPDATE_RECIPES and the
//! unlocked ones by id in RECIPE. The displays are kept here and turned back into recipes; recipe ids
//! are the server's display ids ("000042"), which the client sends back when placing a recipe.

use std::collections::HashMap;

use super::data_1_21 as d;
use super::ids::old;
use super::items::{Items, Reg};
use crate::proto::buf::{DecodeError, Reader, Result, Writer};

fn err(msg: &str) -> DecodeError {
    DecodeError::Other(msg.to_string())
}

/// One converted 1.21 item stack (wire bytes).
type Stack = Vec<u8>;
/// An ingredient: any of these stacks.
type Ingredient = Vec<Stack>;

enum Kind {
    Shapeless { ingredients: Vec<Ingredient>, result: Stack },
    Shaped { width: i32, height: i32, ingredients: Vec<Ingredient>, result: Stack },
    Furnace { ingredient: Ingredient, result: Stack, time: i32, xp: f32 },
}

struct Recipe {
    index: i32,
    group: Option<i32>,
    category: i32,
    highlight: bool,
    kind: Kind,
}

/// Number of recipe book settings: open + filtering for crafting, furnace, blast furnace, smoker.
const SETTINGS: usize = 8;

#[derive(Default)]
pub struct Recipes {
    recipes: Vec<Recipe>,
    stonecutter: Vec<(Ingredient, Stack)>,
    settings: [bool; SETTINGS],
    /// Item tags of the server (1.21.2 ids), to resolve tag slot displays.
    pub item_tags: HashMap<String, Vec<i32>>,
    sent: bool,
}

fn serializer(name: &str) -> i32 {
    d::OLD_RECIPE_SERIALIZER_NAMES.iter().position(|n| *n == name).unwrap_or(0) as i32
}

fn slot_display_name(id: i32) -> &'static str {
    d::NEW_SLOT_DISPLAY_NAMES.get(id.max(0) as usize).copied().unwrap_or("")
}

fn simple_stack(old_item: i32) -> Stack {
    let mut w = Writer::new();
    w.varint(1);
    w.varint(old_item);
    w.varint(0);
    w.varint(0);
    w.into_inner()
}

fn identifier(index: i32) -> String {
    format!("{index:06}")
}

/// 1.21.2 recipe book category → 1.21 crafting / cooking book category.
fn book_category(category: i32, cooking: bool) -> i32 {
    if cooking {
        match category {
            4 | 9 | 12 => 0, // food
            5 | 7 => 1,      // blocks
            _ => 2,          // misc
        }
    } else {
        match category {
            0..=3 => category, // building, redstone, equipment, misc
            _ => 3,
        }
    }
}

impl Recipes {
    /// Reads a slot display; returns the stacks it can show (empty when it cannot be expressed).
    fn slot_display(&self, items: &Items, r: &mut Reader) -> Result<Ingredient> {
        let ty = r.varint()?;
        Ok(match slot_display_name(ty) {
            "empty" | "any_fuel" => Vec::new(),
            "item" => {
                let id = r.varint()?;
                if id == 0 {
                    Vec::new()
                } else {
                    vec![simple_stack(items.item(id))]
                }
            }
            "item_stack" => {
                let mut w = Writer::new();
                items.item_stack(r, &mut w)?;
                if Reader::new(&w.buf).varint()? <= 0 {
                    Vec::new()
                } else {
                    vec![w.into_inner()]
                }
            }
            "tag" => {
                let tag = r.string()?;
                let key = tag.trim_start_matches('#').trim_start_matches("minecraft:");
                self.item_tags.get(key).map(|ids| ids.iter().map(|id| simple_stack(items.item(*id))).collect()).unwrap_or_default()
            }
            "smithing_trim" => {
                for _ in 0..3 {
                    self.slot_display(items, r)?;
                }
                Vec::new()
            }
            "with_remainder" => {
                let input = self.slot_display(items, r)?;
                self.slot_display(items, r)?;
                input
            }
            "composite" => {
                let n = r.varint()?;
                let mut all = Vec::new();
                for _ in 0..n.max(0) {
                    all.extend(self.slot_display(items, r)?);
                }
                all
            }
            other => return Err(err(&format!("unknown slot display {other}"))),
        })
    }

    fn slot_display_list(&self, items: &Items, r: &mut Reader) -> Result<Vec<Ingredient>> {
        let n = r.varint()?;
        (0..n.max(0)).map(|_| self.slot_display(items, r)).collect()
    }

    fn single(&self, items: &Items, r: &mut Reader) -> Result<Stack> {
        let mut stacks = self.slot_display(items, r)?;
        Ok(if stacks.is_empty() { simple_stack(1) } else { stacks.swap_remove(0) })
    }

    /// One RECIPE_BOOK_ADD entry.
    fn read_entry(&self, items: &Items, r: &mut Reader) -> Result<Option<Recipe>> {
        let index = r.varint()?;
        let display = r.varint()?;
        let kind = match display {
            0 => {
                let ingredients = self.slot_display_list(items, r)?;
                let result = self.single(items, r)?;
                self.slot_display(items, r)?; // crafting station
                Some(Kind::Shapeless { ingredients, result })
            }
            1 => {
                let width = r.varint()?;
                let height = r.varint()?;
                let ingredients = self.slot_display_list(items, r)?;
                let result = self.single(items, r)?;
                self.slot_display(items, r)?;
                (width * height == ingredients.len() as i32).then_some(Kind::Shaped { width, height, ingredients, result })
            }
            2 => {
                let ingredient = self.slot_display(items, r)?;
                self.slot_display(items, r)?; // fuel
                let result = self.single(items, r)?;
                self.slot_display(items, r)?;
                let time = r.varint()?;
                let xp = r.f32()?;
                Some(Kind::Furnace { ingredient, result, time, xp })
            }
            3 => {
                // stonecutter recipes come from UPDATE_RECIPES
                for _ in 0..3 {
                    self.slot_display(items, r)?;
                }
                None
            }
            4 => {
                for _ in 0..5 {
                    self.slot_display(items, r)?;
                }
                None
            }
            other => return Err(err(&format!("unknown recipe display {other}"))),
        };
        let group = r.varint()?;
        let category = r.varint()?;
        if r.bool()? {
            let n = r.varint()?;
            for _ in 0..n.max(0) {
                items.holder_set(r, &mut Writer::new(), Reg::Same)?;
            }
        }
        let flags = r.u8()?;
        Ok(kind.map(|kind| Recipe { index, group: (group > 0).then_some(group - 1), category, highlight: flags & 2 != 0, kind }))
    }

    /// RECIPE_BOOK_ADD → UPDATE_RECIPES + RECIPE (init with everything known).
    pub fn recipe_book_add(&mut self, items: &Items, payload: &[u8], out: &mut Vec<(i32, Vec<u8>)>) -> Result<()> {
        let mut r = Reader::new(payload);
        let n = r.varint()?;
        let mut added = Vec::new();
        for _ in 0..n.max(0) {
            if let Some(recipe) = self.read_entry(items, &mut r)? {
                added.push(recipe);
            }
        }
        if r.bool()? {
            self.recipes.clear();
        }
        for recipe in added {
            self.recipes.retain(|x| x.index != recipe.index);
            self.recipes.push(recipe);
        }
        self.send(out);
        Ok(())
    }

    /// RECIPE_BOOK_REMOVE → RECIPE (remove).
    pub fn recipe_book_remove(&mut self, payload: &[u8]) -> Result<Vec<u8>> {
        let mut r = Reader::new(payload);
        let ids = r.list(|r| r.varint())?;
        self.recipes.retain(|x| !ids.contains(&x.index));
        let mut w = Writer::new();
        w.varint(2);
        self.write_settings(&mut w);
        w.varint(ids.len() as i32);
        for id in ids {
            w.string(&identifier(id));
        }
        Ok(w.into_inner())
    }

    pub fn recipe_book_settings(&mut self, payload: &[u8]) -> Result<()> {
        let mut r = Reader::new(payload);
        for s in &mut self.settings {
            *s = r.bool()?;
        }
        Ok(())
    }

    /// UPDATE_RECIPES 1.21.2 (property sets, stonecutter list): kept until the recipe book is sent.
    pub fn update_recipes(&mut self, items: &Items, payload: &[u8], out: &mut Vec<(i32, Vec<u8>)>) -> Result<()> {
        let mut r = Reader::new(payload);
        let n = r.varint()?;
        for _ in 0..n.max(0) {
            r.string()?;
            r.list(|r| r.varint())?;
        }
        self.stonecutter.clear();
        let n = r.varint()?;
        for _ in 0..n.max(0) {
            let v = r.varint()?;
            let ingredient = if v == 0 {
                let tag = r.string()?.to_string();
                let key = tag.trim_start_matches('#').trim_start_matches("minecraft:");
                self.item_tags.get(key).map(|ids| ids.iter().map(|id| simple_stack(items.item(*id))).collect()).unwrap_or_default()
            } else {
                (0..v - 1).map(|_| r.varint().map(|id| simple_stack(items.item(id)))).collect::<Result<Vec<_>>>()?
            };
            let result = self.single(items, &mut r)?;
            self.stonecutter.push((ingredient, result));
        }
        if self.sent {
            self.send(out);
        }
        Ok(())
    }

    fn write_settings(&self, w: &mut Writer) {
        for s in self.settings {
            w.bool(s);
        }
    }

    fn write_ingredient(w: &mut Writer, ingredient: &Ingredient) {
        w.varint(ingredient.len() as i32);
        for s in ingredient {
            w.raw(s);
        }
    }

    fn write_group(w: &mut Writer, group: Option<i32>) {
        w.string(&group.map(|g| g.to_string()).unwrap_or_default());
    }

    fn send(&mut self, out: &mut Vec<(i32, Vec<u8>)>) {
        self.sent = true;
        let highest = self.recipes.iter().map(|r| r.index).max().unwrap_or(-1);
        let mut update = Writer::new();
        let mut order: Vec<&Recipe> = self.recipes.iter().collect();
        order.sort_by_key(|r| r.index);
        update.varint((order.len() + self.stonecutter.len()) as i32);
        for recipe in &order {
            update.string(&identifier(recipe.index));
            match &recipe.kind {
                Kind::Shapeless { ingredients, result } => {
                    update.varint(serializer("crafting_shapeless"));
                    Self::write_group(&mut update, recipe.group);
                    update.varint(book_category(recipe.category, false));
                    update.varint(ingredients.len() as i32);
                    for i in ingredients {
                        Self::write_ingredient(&mut update, i);
                    }
                    update.raw(result);
                }
                Kind::Shaped { width, height, ingredients, result } => {
                    update.varint(serializer("crafting_shaped"));
                    Self::write_group(&mut update, recipe.group);
                    update.varint(book_category(recipe.category, false));
                    update.varint(*width);
                    update.varint(*height);
                    for i in ingredients {
                        Self::write_ingredient(&mut update, i);
                    }
                    update.raw(result);
                    update.bool(false); // show notification
                }
                Kind::Furnace { ingredient, result, time, xp } => {
                    let kind = match recipe.category {
                        7 | 8 => "blasting",
                        9 => "smoking",
                        12 => "campfire_cooking",
                        _ => "smelting",
                    };
                    update.varint(serializer(kind));
                    Self::write_group(&mut update, recipe.group);
                    update.varint(book_category(recipe.category, true));
                    Self::write_ingredient(&mut update, ingredient);
                    update.raw(result);
                    update.f32(*xp);
                    update.varint(*time);
                }
            }
        }
        for (i, (ingredient, result)) in self.stonecutter.iter().enumerate() {
            update.string(&identifier(highest + 1 + i as i32));
            update.varint(serializer("stonecutting"));
            update.string("");
            Self::write_ingredient(&mut update, ingredient);
            update.raw(result);
        }
        out.push((old::UPDATE_RECIPES, update.into_inner()));

        let mut book = Writer::new();
        book.varint(0); // init
        self.write_settings(&mut book);
        book.varint(order.len() as i32);
        for recipe in &order {
            book.string(&identifier(recipe.index));
        }
        let highlighted: Vec<&&Recipe> = order.iter().filter(|r| r.highlight).collect();
        book.varint(highlighted.len() as i32);
        for recipe in highlighted {
            book.string(&identifier(recipe.index));
        }
        out.push((old::RECIPE, book.into_inner()));
    }

    /// Serverbound recipe id string ("minecraft:000042") → display id.
    pub fn display_id(recipe: &str) -> Option<i32> {
        recipe.rsplit(':').next()?.parse().ok()
    }

    /// PLACE_RECIPE 1.21 (byte container, recipe id, shift) → 1.21.2 (varint container, display id).
    pub fn place_recipe(payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let container = r.i8()?;
        let Some(id) = Self::display_id(r.string()?) else { return Ok(None) };
        let mut w = Writer::new();
        w.varint(i32::from(container));
        w.varint(id);
        w.raw(r.remaining());
        Ok(Some(w.into_inner()))
    }

    pub fn seen_recipe(payload: &[u8]) -> Result<Option<Vec<u8>>> {
        let mut r = Reader::new(payload);
        let Some(id) = Self::display_id(r.string()?) else { return Ok(None) };
        let mut w = Writer::new();
        w.varint(id);
        Ok(Some(w.into_inner()))
    }

    #[cfg(test)]
    fn count(&self) -> usize {
        self.recipes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::super::items::Dir;
    use super::*;

    fn display(name: &str) -> i32 {
        d::NEW_SLOT_DISPLAY_NAMES.iter().position(|n| *n == name).unwrap() as i32
    }

    #[test]
    fn shaped_display_becomes_recipe() {
        let items = Items::new(Dir::ToClient);
        let stick = Items::new_item_by_name("stick").unwrap();
        let planks = Items::new_item_by_name("oak_planks").unwrap();
        let mut p = Writer::new();
        p.varint(1); // one entry
        p.varint(42); // display id
        p.varint(1); // shaped
        p.varint(1).varint(2);
        p.varint(2);
        p.varint(display("item")).varint(planks);
        p.varint(display("item")).varint(planks);
        p.varint(display("item_stack")).varint(4).varint(stick).varint(0).varint(0);
        p.varint(display("empty"));
        p.varint(0); // no group
        p.varint(3); // crafting misc
        p.bool(false);
        p.u8(2); // highlight
        p.bool(true); // replace
        let mut rec = Recipes::default();
        let mut out = Vec::new();
        rec.recipe_book_add(&items, &p.buf, &mut out).unwrap();
        assert_eq!(rec.count(), 1);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, old::UPDATE_RECIPES);
        let mut r = Reader::new(&out[0].1);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.string().unwrap(), "000042");
        assert_eq!(r.varint().unwrap(), serializer("crafting_shaped"));
        assert_eq!(r.string().unwrap(), "");
        assert_eq!(r.varint().unwrap(), 3);
        assert_eq!(r.varint().unwrap(), 1);
        assert_eq!(r.varint().unwrap(), 2);
        for _ in 0..2 {
            assert_eq!(r.varint().unwrap(), 1);
            assert_eq!(r.bytes(4).unwrap(), &simple_stack(items.item(planks))[..]);
        }
        assert_eq!(r.varint().unwrap(), 4); // result count
        assert_eq!(r.varint().unwrap(), items.item(stick));
        r.bytes(2).unwrap();
        assert!(!r.bool().unwrap());
        assert!(r.is_empty());
        let mut b = Reader::new(&out[1].1);
        assert_eq!(b.varint().unwrap(), 0);
        b.bytes(SETTINGS).unwrap();
        assert_eq!(b.varint().unwrap(), 1);
        assert_eq!(b.string().unwrap(), "000042");
        assert_eq!(b.varint().unwrap(), 1);
        assert_eq!(b.string().unwrap(), "000042");
        assert!(b.is_empty());
    }

    #[test]
    fn place_recipe_id() {
        let mut p = Writer::new();
        p.i8(1);
        p.string("minecraft:000042");
        p.bool(true);
        let out = Recipes::place_recipe(&p.buf).unwrap().unwrap();
        assert_eq!(out, vec![1, 42, 1]);
        assert_eq!(Recipes::display_id("crafting_table"), None);
    }
}
