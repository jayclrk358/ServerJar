#!/usr/bin/env python3
"""Generates src/translate/data_<ver>.rs: registry id remap tables from a NEW game version to an OLD one.

Input: ViaVersion's generated mapping files (GPL-3.0, like this plugin):
  mappings/mapping-<new>.json, mappings/mapping-<old>.json  -- registry id -> name lists
  mappings/diff/mapping-<new>to<old>.json                   -- fallbacks for entries the old version lacks
Usage: gen_mappings.py <mappings-repo> <new> <old> <out.rs>
"""
import json, sys, re

repo, new, old, out = sys.argv[1:5]
N = json.load(open(f"{repo}/mappings/mapping-{new}.json"))
O = json.load(open(f"{repo}/mappings/mapping-{old}.json"))
D = json.load(open(f"{repo}/mappings/diff/mapping-{new}to{old}.json"))

def apply_materials(name):
    """ViaBackwards "materials": rename a wood/material family, e.g. poplar_planks -> birch_planks."""
    for mat, repl in D.get("materials", {}).items():
        if re.search(rf"(^|_){mat}(_|$)", name):
            return re.sub(rf"(^|_){mat}(_|$)", lambda m: m.group(1) + repl + m.group(2), name)
    return None

def resolve_blockstate(name, old_index):
    base, _, props = name.partition("[")
    props = "[" + props if props else ""
    diff = D.get("blockstates", {})
    candidates = []
    if name in diff:
        candidates.append(diff[name])
    for k, v in diff.items():
        if k == base or k == base + "[":
            # "foo[" means: take foo and keep our properties; "foo" means: that exact state
            candidates.append(v[:-1] + props if v.endswith("[") else v)
    if base in D.get("blocks", {}):
        candidates.append(D["blocks"][base] + props)
    m = apply_materials(base)
    if m:
        candidates.append(m + props)
        if m in D.get("blocks", {}):
            candidates.append(D["blocks"][m] + props)
    for c in candidates:
        if c in old_index:
            return old_index[c]
    for c in candidates:  # try without properties
        cb = c.partition("[")[0]
        if cb in old_index:
            return old_index[cb]
        # first state of that block
        for n2, i2 in old_index.items():
            if n2 == cb or n2.startswith(cb + "["):
                return i2
    return None

def table(key, fallback_default=0, blockstate=False):
    """Returns list new_id -> old_id. Unknown entries use the diff file, else fallback_default."""
    old_index = {name: i for i, name in enumerate(O[key])}
    diff = D.get(key, {})
    result, unmapped = [], []
    for name in N[key]:
        if name in old_index:
            result.append(old_index[name]); continue
        if blockstate:
            r = resolve_blockstate(name, old_index)
            if r is None:
                unmapped.append(name); result.append(fallback_default)
            else:
                result.append(r)
            continue
        target = diff.get(name)
        if target is None:
            target = apply_materials(name)
            if target is not None and target not in old_index and target in diff:
                target = diff[target]
        if target is not None and target in old_index:
            result.append(old_index[target])
        else:
            unmapped.append(name); result.append(fallback_default)
    return result, unmapped

sections = []
stats = []
def emit(name, key, fallback_default=0, blockstate=False):
    if key not in N or key not in O:
        stats.append(f"// {key}: not present in both versions, table omitted"); return
    tbl, unmapped = table(key, fallback_default, blockstate)
    stats.append(f"// {key}: {len(N[key])} ids, {len(unmapped)} with no mapping (fallback {fallback_default}){': ' + ', '.join(unmapped[:8]) + ('...' if len(unmapped) > 8 else '') if unmapped else ''}")
    rows = ",".join(str(x) for x in tbl)
    sections.append(f"pub static {name}: [u32; {len(tbl)}] = [{rows}];")

# For block states the stone fallback (id 1) is less jarring than air.
emit("BLOCK_STATES", "blockstates", fallback_default=1, blockstate=True)
emit("BLOCKS", "blocks", fallback_default=1)
emit("ITEMS", "items", fallback_default=0)
emit("ENTITIES", "entities", fallback_default=O["entities"].index("text_display") if "text_display" in O["entities"] else 0)
emit("PARTICLES", "particles", fallback_default=O["particles"].index("poof") if "poof" in O["particles"] else 0)
emit("SOUNDS", "sounds", fallback_default=0)
emit("BLOCK_ENTITIES", "blockentities", fallback_default=0)
emit("MENUS", "menus", fallback_default=0)
emit("ATTRIBUTES", "attributes", fallback_default=0)
emit("ARGUMENT_TYPES", "argumenttypes", fallback_default=O["argumenttypes"].index("brigadier:string"))
emit("RECIPE_SERIALIZERS", "recipe_serializers", fallback_default=0)
emit("SLOT_DISPLAYS", "slot_displays", fallback_default=0)
sections.append("pub static NEW_ARGUMENT_TYPE_NAMES: [&str; %d] = [%s];" % (len(N["argumenttypes"]), ",".join(json.dumps(n) for n in N["argumenttypes"])))
sections.append("pub static OLD_ARGUMENT_TYPE_NAMES: [&str; %d] = [%s];" % (len(O["argumenttypes"]), ",".join(json.dumps(n) for n in O["argumenttypes"])))
if "slot_displays" in N:
    sections.append("pub static NEW_SLOT_DISPLAY_NAMES: [&str; %d] = [%s];" % (len(N["slot_displays"]), ",".join(json.dumps(n) for n in N["slot_displays"])))
sections.append("pub static NEW_ENTITY_NAMES: [&str; %d] = [%s];" % (len(N["entities"]), ",".join(json.dumps(n) for n in N["entities"])))
sections.append("pub static NEW_PARTICLE_NAMES: [&str; %d] = [%s];" % (len(N["particles"]), ",".join(json.dumps(n) for n in N["particles"])))
sections.append("pub static OLD_PARTICLE_NAMES: [&str; %d] = [%s];" % (len(O["particles"]), ",".join(json.dumps(n) for n in O["particles"])))
removed_env = [k for k, v in D.get("environment_attribute", {}).items() if v == ""]
sections.append("pub static REMOVED_ENVIRONMENT_ATTRIBUTES: [&str; %d] = [%s];" % (len(removed_env), ",".join(json.dumps("minecraft:" + n) for n in removed_env)))

# Data component types: new ones that the old client doesn't know map to u32::MAX (= drop).
old_c = {n: i for i, n in enumerate(O["data_component_type"])}
comp = [old_c.get(n, 0xFFFFFFFF) for n in N["data_component_type"]]
sections.append(f"pub const COMPONENT_DROP: u32 = 0xFFFF_FFFF;\npub static COMPONENTS: [u32; {len(comp)}] = [{','.join(str(x) for x in comp)}];")
# Reverse direction (old client -> new server) for serverbound item stacks.
new_c = {n: i for i, n in enumerate(N["data_component_type"])}
rcomp = [new_c.get(n, 0xFFFFFFFF) for n in O["data_component_type"]]
sections.append(f"pub static COMPONENTS_REVERSE: [u32; {len(rcomp)}] = [{','.join(str(x) for x in rcomp)}];")
def emit_reverse(name, key, fallback=0):
    new_index = {n: i for i, n in enumerate(N[key])}
    rows = [new_index.get(n, fallback) for n in O[key]]
    sections.append(f"pub static {name}_REVERSE: [u32; {len(rows)}] = [{','.join(str(x) for x in rows)}];")
for name, key in [("ITEMS", "items"), ("BLOCKS", "blocks"), ("ENTITIES", "entities"), ("PARTICLES", "particles"), ("SOUNDS", "sounds"), ("ATTRIBUTES", "attributes"), ("ARGUMENT_TYPES", "argumenttypes")]:
    emit_reverse(name, key)
sections.append("pub static OLD_COMPONENT_NAMES: [&str; %d] = [%s];" % (len(O["data_component_type"]), ",".join(json.dumps(n) for n in O["data_component_type"])))
sections.append("pub const BRICK_ITEM_NEW: u32 = %d;\npub const BRICK_ITEM_OLD: u32 = %d;" % (N["items"].index("brick"), O["items"].index("brick")))
sections.append("pub const TEXT_DISPLAY_ENTITY_OLD: u32 = %d;\npub const FALLING_BLOCK_ENTITY_OLD: u32 = %d;" % (O["entities"].index("text_display"), O["entities"].index("falling_block")))
if "cushion" in N["entities"]:
    sections.append("pub const CUSHION_ENTITY_NEW: u32 = %d;" % N["entities"].index("cushion"))
# Names of every registry in both versions, so each step can look ids up by name.
for key, const in [("entities", "OLD_ENTITY_NAMES"), ("items", "NEW_ITEM_NAMES"), ("items", "OLD_ITEM_NAMES"), ("blocks", "NEW_BLOCK_NAMES"), ("blocks", "OLD_BLOCK_NAMES"), ("sounds", "NEW_SOUND_NAMES"), ("sounds", "OLD_SOUND_NAMES"), ("attributes", "NEW_ATTRIBUTE_NAMES"), ("attributes", "OLD_ATTRIBUTE_NAMES"), ("blockentities", "NEW_BLOCK_ENTITY_NAMES"), ("blockentities", "OLD_BLOCK_ENTITY_NAMES")]:
    src = N if const.startswith("NEW") else O
    if key in src:
        sections.append("pub static %s: [&str; %d] = [%s];" % (const, len(src[key]), ",".join(json.dumps(n) for n in src[key])))
for key, const in [("recipe_serializers", "NEW_RECIPE_SERIALIZER_NAMES"), ("recipe_serializers", "OLD_RECIPE_SERIALIZER_NAMES"), ("menus", "NEW_MENU_NAMES"), ("menus", "OLD_MENU_NAMES"), ("enchantments", "NEW_ENCHANTMENT_NAMES"), ("enchantments", "OLD_ENCHANTMENT_NAMES")]:
    src = N if const.startswith("NEW") else O
    if key in src:
        sections.append("pub static %s: [&str; %d] = [%s];" % (const, len(src[key]), ",".join(json.dumps(n) for n in src[key])))
sections.append("pub const WHITE_CARPET_STATE_OLD: u32 = %d;" % O["blockstates"].index("white_carpet"))
new_bs = {n: i for i, n in enumerate(N["blockstates"])}
rbs = [new_bs.get(n, 0) for n in O["blockstates"]]
sections.append(f"pub static BLOCK_STATES_REVERSE: [u32; {len(rbs)}] = [{','.join(str(x) for x in rbs)}];")
sections.append(f"pub const NEW_BLOCK_STATE_COUNT: u32 = {len(N['blockstates'])};\npub const OLD_BLOCK_STATE_COUNT: u32 = {len(O['blockstates'])};\npub const OLD_BIOME_COUNT_HINT: u32 = 64;")
# Component names for both versions (useful for logging / NBT rewriting).
sections.append("pub static NEW_COMPONENT_NAMES: [&str; %d] = [%s];" % (len(N["data_component_type"]), ",".join(json.dumps(n) for n in N["data_component_type"])))
stats.append(f"// data_component_type: {len(N['data_component_type'])} -> {len(O['data_component_type'])}, dropped: {', '.join(n for n in N['data_component_type'] if n not in old_c)}")

header = f"""// Generated by tools/gen_mappings.py from ViaVersion's mapping files (GPL-3.0). Do not edit.
// Direction: Minecraft {new} (server) -> Minecraft {old} (client). Index = {new} id, value = {old} id.
#![allow(clippy::all, dead_code)]
"""
with open(out, "w") as f:
    f.write(header + "\n".join(stats) + "\n\n" + "\n".join(sections) + "\n")
print("\n".join(stats))
